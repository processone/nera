//! Unused variable detection.

use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{self, AtomId, Pattern};
use nera_syntax::error::Diagnostic;

use super::ast_walkers::{
    collect_enclosing_refs, collect_expr_refs, collect_guard_refs, collect_pattern_bindings,
    collect_pattern_bindings_with_spans, collect_pattern_expr_refs, collect_pattern_local_refs,
    collect_pattern_refs,
};
use super::unbound_vars::collect_exported_bindings;

#[cfg(test)]
#[path = "unused_vars_tests.rs"]
mod tests;

/// Check a function for unused variables in all its clauses.
pub(super) fn check_unused_variables_fn(
    func: &ast::Function,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    let empty_scope = HashSet::new();
    for clause in &func.clauses {
        check_clause_unused_vars(
            &clause.patterns,
            clause.guard.as_ref(),
            &clause.body,
            atom_names,
            warnings,
            &empty_scope,
            &empty_scope,
            &empty_scope,
        );
    }
}

/// Check a single clause (function head / case arm / receive arm) for unused
/// pattern-bound variables, then recurse into nested clauses in the body.
///
/// `outer_scope` holds the names bound before the clause. `compared` holds
/// the names its patterns compare against instead of binding: the outer
/// scope for case, receive, try and maybe arms, and nothing for function and
/// fun heads, which shadow outer names. `used_after` contains reads after the
/// enclosing expression, restricted to names that expression exports.
#[expect(
    clippy::too_many_arguments,
    reason = "Clause patterns, scope and later reads are distinct inputs to unused-variable analysis"
)]
fn check_clause_unused_vars(
    patterns: &[Pattern],
    guard: Option<&ast::Guard>,
    body: &[ast::Expr],
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
    compared: &HashSet<AtomId>,
    outer_scope: &HashSet<AtomId>,
    used_after: &HashSet<AtomId>,
) {
    // 1. Collect all variable bindings from patterns (with spans)
    let mut bound: Vec<(AtomId, nera_syntax::span::Span)> = Vec::new();
    for pat in patterns {
        collect_pattern_bindings_with_spans(pat, &mut bound);
    }

    // 2. Filter out variables already bound in outer scope — they appear in the
    //    pattern for equality checking, not as fresh bindings.
    bound.retain(|(atom_id, _)| !compared.contains(atom_id));

    // 3. Detect variables appearing more than once in the patterns.
    //    In Erlang, a repeated variable in patterns is an equality constraint
    //    (e.g. `foo({tcp, Socket, _}, #state{socket=Socket})` requires both
    //    Sockets to match).  These variables are implicitly "used" for matching
    //    and should not be flagged as unused.
    let mut seen_once: HashSet<AtomId> = HashSet::new();
    let mut duplicated: HashSet<AtomId> = HashSet::new();
    for (atom_id, _) in &bound {
        if !seen_once.insert(*atom_id) {
            duplicated.insert(*atom_id);
        }
    }

    // 4. Collect reads of local pattern bindings, then guard and body reads.
    //    Pattern inputs read outer bindings, not new (possibly shadowing)
    //    values in this clause; the enclosing expression collects those inputs.
    let mut referenced = HashSet::new();
    for pat in patterns {
        collect_pattern_local_refs(pat, &mut referenced);
    }
    if let Some(guard) = guard {
        collect_guard_refs(guard, &mut referenced);
    }
    for expr in body {
        collect_expr_refs(expr, &mut referenced);
    }

    referenced.extend(used_after);

    // 5. Emit warning for each bound var not in referenced set
    //    and not used for equality matching (duplicated).
    for (atom_id, span) in &bound {
        if referenced.contains(atom_id) || duplicated.contains(atom_id) {
            continue;
        }
        let name = match atom_names.get(atom_id) {
            Some(s) => s.as_str(),
            // Can't resolve name — skip warning (may be _-prefixed)
            None => continue,
        };
        // Skip _-prefixed variables (Erlang convention for intentionally unused)
        if name.starts_with('_') {
            continue;
        }
        warnings.push(
            Diagnostic::warning_at(format!("variable '{}' is unused", name), *span)
                .with_code("unused_variable"),
        );
    }

    // 6. Build nested scope (outer + newly bound) for recursion into body.
    let mut nested_scope = outer_scope.clone();
    for (atom_id, _) in &bound {
        nested_scope.insert(*atom_id);
    }

    // 7. Recurse into case/receive/try/fun clauses nested in body. Walking
    //    in order, `X = 42, case ... of {msg, X} -> ...` sees X in scope.
    check_unused_in_body(body, atom_names, warnings, &mut nested_scope, used_after);
}

/// Walk a body in order: each expression sees the names earlier ones bind,
/// including names bound in a block, a branch or a receive timeout. Later
/// reads flow back into nested branches as well as direct matches.
fn check_unused_in_body(
    body: &[ast::Expr],
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
    scope: &mut HashSet<AtomId>,
    used_after: &HashSet<AtomId>,
) {
    with_continuation_uses(
        body,
        used_after,
        collect_enclosing_refs,
        |expr, later_refs| {
            if matches!(expr, ast::Expr::Match(..)) {
                warn_unused_match_bindings(expr, later_refs, scope, atom_names, warnings);
            }
            check_unused_in_expr(expr, atom_names, warnings, scope, later_refs);
            collect_exported_bindings(expr, scope);
        },
    );
}

/// Recursively walk an expression tree looking for nested clause constructs
/// (case, receive, try, fun) and check each for unused pattern variables.
fn check_unused_in_expr(
    expr: &ast::Expr,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
    outer_scope: &HashSet<AtomId>,
    used_after: &HashSet<AtomId>,
) {
    // Filter only at branch boundaries, not at every node of a literal tree.
    // Scope-local constructs cannot consume reads from outside their scope.
    let mut exported_uses = HashSet::new();
    let used_after = match expr {
        ast::Expr::Case(..) | ast::Expr::If(..) | ast::Expr::Receive(..)
            if !used_after.is_empty() =>
        {
            collect_exported_bindings(expr, &mut exported_uses);
            exported_uses.retain(|id| used_after.contains(id));
            &exported_uses
        }
        ast::Expr::Fun(..)
        | ast::Expr::ListComp(..)
        | ast::Expr::BinComp(..)
        | ast::Expr::Try { .. }
        | ast::Expr::Maybe { .. }
        | ast::Expr::Catch(..) => &exported_uses,
        _ => used_after,
    };

    match expr {
        ast::Expr::Case(scrutinee, clauses, _) => {
            let mut input_uses = used_after.clone();
            collect_clause_refs(clauses, &mut input_uses);
            check_unused_in_expr(scrutinee, atom_names, warnings, outer_scope, &input_uses);
            // Names the scrutinee binds are bound in every clause.
            let mut input = outer_scope.clone();
            collect_exported_bindings(scrutinee, &mut input);
            for clause in clauses {
                check_clause_unused_vars(
                    std::slice::from_ref(&clause.pattern),
                    clause.guard.as_ref(),
                    &clause.body,
                    atom_names,
                    warnings,
                    &input,
                    &input,
                    used_after,
                );
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                check_clause_unused_vars(
                    std::slice::from_ref(&clause.pattern),
                    clause.guard.as_ref(),
                    &clause.body,
                    atom_names,
                    warnings,
                    outer_scope,
                    outer_scope,
                    used_after,
                );
            }
            if let Some(after) = after {
                // As in erlc, the after body does not see the timeout's
                // bindings.
                check_unused_in_body(
                    &after.body,
                    atom_names,
                    warnings,
                    &mut outer_scope.clone(),
                    used_after,
                );
            }
        }
        ast::Expr::Try {
            body,
            of_clauses,
            catch_clauses,
            after_body,
            ..
        } => {
            // The of-clauses see the try body's bindings.
            let mut try_scope = outer_scope.clone();
            let mut of_uses = HashSet::new();
            collect_clause_refs(of_clauses, &mut of_uses);
            check_unused_in_body(body, atom_names, warnings, &mut try_scope, &of_uses);
            for clause in of_clauses {
                check_clause_unused_vars(
                    std::slice::from_ref(&clause.pattern),
                    clause.guard.as_ref(),
                    &clause.body,
                    atom_names,
                    warnings,
                    &try_scope,
                    &try_scope,
                    used_after,
                );
            }
            for clause in catch_clauses {
                // The class and stacktrace names are bound in the body.
                let mut catch_scope = outer_scope.clone();
                if let ast::CatchClass::Variable(id, _) = clause.class {
                    if atom_names.get(&id).is_some_and(|name| name != "_") {
                        catch_scope.insert(id);
                    }
                }
                if let Some(stacktrace) = &clause.stacktrace {
                    collect_pattern_bindings(stacktrace, &mut catch_scope);
                }
                check_clause_unused_vars(
                    std::slice::from_ref(&clause.pattern),
                    clause.guard.as_ref(),
                    &clause.body,
                    atom_names,
                    warnings,
                    outer_scope,
                    &catch_scope,
                    used_after,
                );
            }
            if let Some(after) = after_body {
                check_unused_in_body(
                    after,
                    atom_names,
                    warnings,
                    &mut outer_scope.clone(),
                    used_after,
                );
            }
        }
        ast::Expr::Fun(name, clauses, _) => {
            // Fun heads bind fresh names, shadowing outer ones; the bodies
            // still see captured names and the fun's own name.
            let fresh = HashSet::new();
            let mut fun_scope = outer_scope.clone();
            if let Some(name) = name {
                fun_scope.insert(*name);
            }
            for clause in clauses {
                check_clause_unused_vars(
                    &clause.patterns,
                    clause.guard.as_ref(),
                    &clause.body,
                    atom_names,
                    warnings,
                    &fresh,
                    &fun_scope,
                    used_after,
                );
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                check_unused_in_body(
                    &clause.body,
                    atom_names,
                    warnings,
                    &mut outer_scope.clone(),
                    used_after,
                );
            }
        }
        // Recurse into sub-expressions for completeness
        ast::Expr::Block(exprs, _) => {
            check_unused_in_body(
                exprs,
                atom_names,
                warnings,
                &mut outer_scope.clone(),
                used_after,
            );
        }
        ast::Expr::Tuple(exprs, _) => {
            for e in exprs {
                check_unused_in_expr(e, atom_names, warnings, outer_scope, used_after);
            }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) | ast::Expr::BinOp(_, h, t, _) => {
            check_unused_in_expr(h, atom_names, warnings, outer_scope, used_after);
            check_unused_in_expr(t, atom_names, warnings, outer_scope, used_after);
        }
        ast::Expr::Match(pat, e, _) => {
            let mut pattern_uses = used_after.clone();
            collect_pattern_bindings(pat, &mut pattern_uses);
            collect_pattern_expr_refs(pat, &mut pattern_uses);
            check_unused_in_expr(e, atom_names, warnings, outer_scope, &pattern_uses);
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            check_unused_in_expr(e, atom_names, warnings, outer_scope, used_after);
        }
        ast::Expr::Call(target, args, _) => {
            if let ast::CallTarget::Remote(module_expr, fun_expr) = target {
                check_unused_in_expr(module_expr, atom_names, warnings, outer_scope, used_after);
                check_unused_in_expr(fun_expr, atom_names, warnings, outer_scope, used_after);
            }
            for arg in args {
                check_unused_in_expr(arg, atom_names, warnings, outer_scope, used_after);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_unused_in_expr(base, atom_names, warnings, outer_scope, used_after);
            for field in fields {
                check_unused_in_expr(&field.key, atom_names, warnings, outer_scope, used_after);
                check_unused_in_expr(&field.value, atom_names, warnings, outer_scope, used_after);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            check_unused_in_expr(base, atom_names, warnings, outer_scope, used_after);
            for field in fields {
                check_unused_in_expr(&field.value, atom_names, warnings, outer_scope, used_after);
            }
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for field in fields {
                check_unused_in_expr(&field.value, atom_names, warnings, outer_scope, used_after);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            check_unused_in_expr(base, atom_names, warnings, outer_scope, used_after);
        }
        ast::Expr::ListComp(body, qualifiers, _) | ast::Expr::BinComp(body, qualifiers, _) => {
            // Generator patterns bind names for later qualifiers and the body.
            let mut later_refs = HashSet::new();
            collect_enclosing_refs(body, &mut later_refs);
            let mut continuations = Vec::with_capacity(qualifiers.len());
            for q in qualifiers.iter().rev() {
                match q {
                    ast::LcQualifier::Generator(pat, expr, _)
                    | ast::LcQualifier::BinGenerator(pat, expr, _) => {
                        // The generator head shadows earlier names, but its
                        // source and pattern inputs still read those names.
                        let mut fresh = HashSet::new();
                        collect_pattern_bindings(pat, &mut fresh);
                        later_refs.retain(|id| !fresh.contains(id));
                        collect_pattern_refs(pat, &mut later_refs);
                        continuations.push(later_refs.clone());
                        collect_enclosing_refs(expr, &mut later_refs);
                    }
                    ast::LcQualifier::Filter(expr, _) => {
                        continuations.push(later_refs.clone());
                        collect_enclosing_refs(expr, &mut later_refs);
                    }
                }
            }
            let mut lc_scope = outer_scope.clone();
            for (q, later_refs) in qualifiers.iter().zip(continuations.iter().rev()) {
                match q {
                    ast::LcQualifier::Generator(pat, expr, _)
                    | ast::LcQualifier::BinGenerator(pat, expr, _) => {
                        check_unused_in_expr(expr, atom_names, warnings, &lc_scope, later_refs);
                        collect_pattern_bindings(pat, &mut lc_scope);
                    }
                    ast::LcQualifier::Filter(expr, _) => {
                        check_unused_in_expr(expr, atom_names, warnings, &lc_scope, later_refs);
                        collect_exported_bindings(expr, &mut lc_scope);
                    }
                }
            }
            check_unused_in_expr(body, atom_names, warnings, &lc_scope, used_after);
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                check_unused_in_expr(&seg.value, atom_names, warnings, outer_scope, used_after);
                if let Some(ref size) = seg.size {
                    check_unused_in_expr(size, atom_names, warnings, outer_scope, used_after);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for field in fields {
                check_unused_in_expr(&field.key, atom_names, warnings, outer_scope, used_after);
                check_unused_in_expr(&field.value, atom_names, warnings, outer_scope, used_after);
            }
        }
        ast::Expr::Maybe {
            body, else_clauses, ..
        } => {
            let mut maybe_scope = outer_scope.clone();
            with_continuation_uses(
                body,
                used_after,
                |item, refs| match item {
                    ast::MaybeExpr::Expr(e) => collect_enclosing_refs(e, refs),
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        // Already-bound value names are comparison inputs too.
                        collect_pattern_bindings(pat, refs);
                        collect_pattern_expr_refs(pat, refs);
                        collect_enclosing_refs(e, refs);
                    }
                },
                |item, later_refs| match item {
                    ast::MaybeExpr::Expr(e) => {
                        check_unused_in_expr(e, atom_names, warnings, &maybe_scope, later_refs);
                        collect_exported_bindings(e, &mut maybe_scope);
                    }
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        let mut pattern_uses = later_refs.clone();
                        collect_pattern_bindings(pat, &mut pattern_uses);
                        collect_pattern_expr_refs(pat, &mut pattern_uses);
                        check_unused_in_expr(e, atom_names, warnings, &maybe_scope, &pattern_uses);
                        collect_exported_bindings(e, &mut maybe_scope);
                        collect_pattern_bindings(pat, &mut maybe_scope);
                    }
                },
            );
            for clause in else_clauses {
                check_clause_unused_vars(
                    std::slice::from_ref(&clause.pattern),
                    clause.guard.as_ref(),
                    &clause.body,
                    atom_names,
                    warnings,
                    outer_scope,
                    outer_scope,
                    used_after,
                );
            }
        }
        // Leaf nodes — nothing to recurse into
        ast::Expr::Variable(_, _)
        | ast::Expr::Integer(_, _)
        | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(_, _)
        | ast::Expr::Atom(_, _)
        | ast::Expr::String(_, _)
        | ast::Expr::Nil(_)
        | ast::Expr::FunRef(ast::FunRefTarget::Local(..) | ast::FunRefTarget::Remote(..), _)
        | ast::Expr::RecordIndex { .. } => {}
        ast::Expr::FunRef(ast::FunRefTarget::DynamicRemote(m, f, a), _) => {
            check_unused_in_expr(m, atom_names, warnings, outer_scope, used_after);
            check_unused_in_expr(f, atom_names, warnings, outer_scope, used_after);
            check_unused_in_expr(a, atom_names, warnings, outer_scope, used_after);
        }
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    check_unused_in_expr(arg, atom_names, warnings, outer_scope, used_after);
                }
            }
        }
    }
}

/// Visit a sequence with its remaining reads. Count each item's reads once,
/// then subtract them in order, without rescanning or copying whole suffixes.
fn with_continuation_uses<T>(
    items: &[T],
    used_after: &HashSet<AtomId>,
    collect: impl Fn(&T, &mut HashSet<AtomId>),
    mut visit: impl FnMut(&T, &HashSet<AtomId>),
) {
    let reads: Vec<_> = items
        .iter()
        .map(|item| {
            let mut refs = HashSet::new();
            collect(item, &mut refs);
            refs
        })
        .collect();
    let mut counts = HashMap::<AtomId, usize>::new();
    for id in used_after.iter().chain(reads.iter().flatten()) {
        *counts.entry(*id).or_default() += 1;
    }
    let mut remaining = counts.keys().copied().collect::<HashSet<_>>();
    for (item, refs) in items.iter().zip(reads) {
        for id in refs {
            let count = counts.get_mut(&id).expect("counted read");
            *count -= 1;
            if *count == 0 {
                remaining.remove(&id);
            }
        }
        visit(item, &remaining);
    }
}

/// Reads in case/try-of clauses can consume bindings from their input.
fn collect_clause_refs(clauses: &[ast::CaseClause], refs: &mut HashSet<AtomId>) {
    for clause in clauses {
        collect_pattern_bindings(&clause.pattern, refs);
        collect_pattern_expr_refs(&clause.pattern, refs);
        if let Some(guard) = &clause.guard {
            collect_guard_refs(guard, refs);
        }
        for expr in &clause.body {
            collect_enclosing_refs(expr, refs);
        }
    }
}

/// Collect bindings with spans and reads in the patterns of a (possibly
/// chained) match. A size can use an earlier segment bound in this same match.
fn collect_match_bindings_and_refs(
    expr: &ast::Expr,
    bound: &mut Vec<(AtomId, nera_syntax::span::Span)>,
    refs: &mut HashSet<AtomId>,
) {
    if let ast::Expr::Match(pat, rhs, _) = expr {
        collect_pattern_bindings_with_spans(pat, bound);
        collect_pattern_expr_refs(pat, refs);
        collect_match_bindings_and_refs(rhs, bound, refs);
    }
}

/// Warn about each name a body-level match binds fresh that neither the
/// match itself nor a later expression reads.
fn warn_unused_match_bindings(
    expr: &ast::Expr,
    later_refs: &HashSet<AtomId>,
    in_scope: &HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    let mut new_bindings: Vec<(AtomId, nera_syntax::span::Span)> = Vec::new();
    let mut referenced = HashSet::new();
    collect_match_bindings_and_refs(expr, &mut new_bindings, &mut referenced);

    // Filter to only NEW bindings (not already in scope).
    new_bindings.retain(|(atom_id, _)| !in_scope.contains(atom_id));

    if new_bindings.is_empty() {
        return;
    }

    // Add references from subsequent expressions to the same-match reads.
    referenced.extend(later_refs);

    let mut warned: HashSet<AtomId> = HashSet::new();
    for (atom_id, span) in &new_bindings {
        if warned.contains(atom_id) || referenced.contains(atom_id) {
            continue;
        }
        let name = match atom_names.get(atom_id) {
            Some(s) => s.as_str(),
            None => continue,
        };
        if name.starts_with('_') {
            continue;
        }
        warnings.push(
            Diagnostic::warning_at(format!("variable '{}' is unused", name), *span)
                .with_code("unused_variable"),
        );
        warned.insert(*atom_id);
    }
}
