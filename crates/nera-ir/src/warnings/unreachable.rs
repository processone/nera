//! Unreachable clause detection.
//!
//! A clause is unreachable after a guardless clause that matches anything.
//! A pattern variable matches anything only when it is fresh: a name bound
//! before the clause is a comparison, as in erlc. Function and fun heads
//! always bind fresh names, shadowing outer ones.

use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{self, AtomId, Pattern};
use nera_syntax::error::Diagnostic;

use super::ast_walkers::{collect_pattern_bindings, collect_pattern_var_ids};
use super::unbound_vars::collect_exported_bindings;

/// Check a function for unreachable clauses.
pub(super) fn check_unreachable_clauses_fn(
    func: &ast::Function,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    let func_name = atom_names
        .get(&func.name)
        .map(|s| s.as_str())
        .unwrap_or("?");

    // Check function clauses
    let mut seen_catch_all = false;
    for clause in &func.clauses {
        if seen_catch_all {
            warnings.push(Diagnostic::warning_at(
                format!(
                    "this clause in '{}/{}' cannot be reached",
                    func_name, func.arity
                ),
                clause.span,
            ).with_code("unreachable_clause"));
        }
        if clause.guard.is_none() && is_patterns_catch_all(&clause.patterns) {
            seen_catch_all = true;
        }
    }

    // Recurse into bodies for nested case/receive
    for clause in &func.clauses {
        let mut scope = HashSet::new();
        for pat in &clause.patterns {
            collect_pattern_bindings(pat, &mut scope);
        }
        check_unreachable_in_body(&clause.body, &mut scope, atom_names, warnings);
    }
}

/// Walk a body in order: each expression sees the names earlier ones bind.
fn check_unreachable_in_body(
    body: &[ast::Expr],
    scope: &mut HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    for expr in body {
        check_unreachable_in_expr(expr, scope, atom_names, warnings);
        collect_exported_bindings(expr, scope);
    }
}

/// Walk a clause body with the clause's pattern bindings added to `scope`.
fn check_clause_body(
    pattern: Option<&Pattern>,
    body: &[ast::Expr],
    scope: &HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    let mut scope = scope.clone();
    if let Some(pattern) = pattern {
        collect_pattern_bindings(pattern, &mut scope);
    }
    check_unreachable_in_body(body, &mut scope, atom_names, warnings);
}

/// Recursively walk an expression looking for case/receive/try/fun with
/// unreachable clauses. `scope` holds the names bound before `expr`.
fn check_unreachable_in_expr(
    expr: &ast::Expr,
    scope: &HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    match expr {
        ast::Expr::Case(scrutinee, clauses, _) => {
            check_unreachable_in_expr(scrutinee, scope, atom_names, warnings);
            // Names the scrutinee binds are bound in every clause.
            let mut input = scope.clone();
            collect_exported_bindings(scrutinee, &mut input);
            let mut seen_catch_all = false;
            for clause in clauses {
                if seen_catch_all {
                    warnings.push(Diagnostic::warning_at(
                        "this clause in 'case' cannot be reached".to_string(),
                        clause.span,
                    ).with_code("unreachable_clause"));
                }
                if clause.guard.is_none() && is_catch_all(&clause.pattern, &input) {
                    seen_catch_all = true;
                }
                let pattern = Some(&clause.pattern);
                check_clause_body(pattern, &clause.body, &input, atom_names, warnings);
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            let mut seen_catch_all = false;
            for clause in clauses {
                if seen_catch_all {
                    warnings.push(Diagnostic::warning_at(
                        "this clause in 'receive' cannot be reached".to_string(),
                        clause.span,
                    ).with_code("unreachable_clause"));
                }
                if clause.guard.is_none() && is_catch_all(&clause.pattern, scope) {
                    seen_catch_all = true;
                }
                check_clause_body(Some(&clause.pattern), &clause.body, scope, atom_names, warnings);
            }
            if let Some(after) = after {
                // As in erlc, the after body does not see the timeout's
                // bindings.
                check_clause_body(None, &after.body, scope, atom_names, warnings);
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
            let mut try_scope = scope.clone();
            check_unreachable_in_body(body, &mut try_scope, atom_names, warnings);
            let mut seen_catch_all = false;
            for clause in of_clauses {
                if seen_catch_all {
                    warnings.push(Diagnostic::warning_at(
                        "this clause in 'try...of' cannot be reached".to_string(),
                        clause.span,
                    ).with_code("unreachable_clause"));
                }
                if clause.guard.is_none() && is_catch_all(&clause.pattern, &try_scope) {
                    seen_catch_all = true;
                }
                let pattern = Some(&clause.pattern);
                check_clause_body(pattern, &clause.body, &try_scope, atom_names, warnings);
            }
            // A guardless catch-all pattern catches every exception of its
            // clause's classes; a later clause is unreachable once every
            // class it matches is caught.
            let mut caught: Vec<Classes> = Vec::new();
            for clause in catch_clauses {
                let classes = catch_classes(&clause.class, scope, atom_names);
                if caught.contains(&Classes::Every)
                    || (classes != Classes::Bound && caught.contains(&classes))
                {
                    warnings.push(Diagnostic::warning_at(
                        "this clause in 'catch' cannot be reached".to_string(),
                        clause.span,
                    ).with_code("unreachable_clause"));
                }
                if clause.guard.is_none() && is_catch_all(&clause.pattern, scope) {
                    caught.push(classes);
                }
                let mut clause_scope = scope.clone();
                if let ast::CatchClass::Variable(id, _) = clause.class {
                    if !is_wildcard_name(id, atom_names) {
                        clause_scope.insert(id);
                    }
                }
                if let Some(stacktrace) = &clause.stacktrace {
                    collect_pattern_bindings(stacktrace, &mut clause_scope);
                }
                let pattern = Some(&clause.pattern);
                check_clause_body(pattern, &clause.body, &clause_scope, atom_names, warnings);
            }
            if let Some(after) = after_body {
                check_clause_body(None, after, scope, atom_names, warnings);
            }
        }
        ast::Expr::Fun(name, clauses, _) => {
            // Fun heads bind fresh names, so a catch-all head does not depend
            // on the enclosing scope; the bodies still see captured names.
            let mut fun_scope = scope.clone();
            if let Some(name) = name {
                fun_scope.insert(*name);
            }
            let mut seen_catch_all = false;
            for clause in clauses {
                if seen_catch_all {
                    warnings.push(Diagnostic::warning_at(
                        "this clause in 'fun' cannot be reached".to_string(),
                        clause.span,
                    ).with_code("unreachable_clause"));
                }
                if clause.guard.is_none() && is_patterns_catch_all(&clause.patterns) {
                    seen_catch_all = true;
                }
                let mut clause_scope = fun_scope.clone();
                for pat in &clause.patterns {
                    collect_pattern_bindings(pat, &mut clause_scope);
                }
                check_unreachable_in_body(&clause.body, &mut clause_scope, atom_names, warnings);
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                check_clause_body(None, &clause.body, scope, atom_names, warnings);
            }
        }
        // Recurse into sub-expressions
        ast::Expr::Block(exprs, _) => {
            check_unreachable_in_body(exprs, &mut scope.clone(), atom_names, warnings);
        }
        ast::Expr::Tuple(exprs, _) => {
            for e in exprs {
                check_unreachable_in_expr(e, scope, atom_names, warnings);
            }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) | ast::Expr::BinOp(_, h, t, _) => {
            check_unreachable_in_expr(h, scope, atom_names, warnings);
            check_unreachable_in_expr(t, scope, atom_names, warnings);
        }
        ast::Expr::Match(_, e, _) | ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            check_unreachable_in_expr(e, scope, atom_names, warnings);
        }
        ast::Expr::Call(target, args, _) => {
            if let ast::CallTarget::Remote(module_expr, fun_expr) = target {
                check_unreachable_in_expr(module_expr, scope, atom_names, warnings);
                check_unreachable_in_expr(fun_expr, scope, atom_names, warnings);
            }
            for arg in args {
                check_unreachable_in_expr(arg, scope, atom_names, warnings);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_unreachable_in_expr(base, scope, atom_names, warnings);
            for f in fields {
                check_unreachable_in_expr(&f.key, scope, atom_names, warnings);
                check_unreachable_in_expr(&f.value, scope, atom_names, warnings);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            check_unreachable_in_expr(base, scope, atom_names, warnings);
            for f in fields {
                check_unreachable_in_expr(&f.value, scope, atom_names, warnings);
            }
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                check_unreachable_in_expr(&f.value, scope, atom_names, warnings);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            check_unreachable_in_expr(base, scope, atom_names, warnings);
        }
        ast::Expr::ListComp(body, qualifiers, _) | ast::Expr::BinComp(body, qualifiers, _) => {
            // Generator patterns bind names for later qualifiers and the body.
            let mut lc_scope = scope.clone();
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(pat, e, _)
                    | ast::LcQualifier::BinGenerator(pat, e, _) => {
                        check_unreachable_in_expr(e, &lc_scope, atom_names, warnings);
                        collect_pattern_bindings(pat, &mut lc_scope);
                    }
                    ast::LcQualifier::Filter(e, _) => {
                        check_unreachable_in_expr(e, &lc_scope, atom_names, warnings);
                    }
                }
            }
            check_unreachable_in_expr(body, &lc_scope, atom_names, warnings);
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                check_unreachable_in_expr(&seg.value, scope, atom_names, warnings);
                if let Some(ref size) = seg.size {
                    check_unreachable_in_expr(size, scope, atom_names, warnings);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                check_unreachable_in_expr(&f.key, scope, atom_names, warnings);
                check_unreachable_in_expr(&f.value, scope, atom_names, warnings);
            }
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            let mut maybe_scope = scope.clone();
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => {
                        check_unreachable_in_expr(e, &maybe_scope, atom_names, warnings);
                        collect_exported_bindings(e, &mut maybe_scope);
                    }
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        check_unreachable_in_expr(e, &maybe_scope, atom_names, warnings);
                        collect_exported_bindings(e, &mut maybe_scope);
                        collect_pattern_bindings(pat, &mut maybe_scope);
                    }
                }
            }
            for clause in else_clauses {
                check_clause_body(Some(&clause.pattern), &clause.body, scope, atom_names, warnings);
            }
        }
        // Leaf nodes
        ast::Expr::Variable(_, _)
        | ast::Expr::Integer(_, _)
        | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(_, _)
        | ast::Expr::Atom(_, _)
        | ast::Expr::String(_, _)
        | ast::Expr::Nil(_)
        | ast::Expr::FunRef(_, _)
        | ast::Expr::RecordIndex { .. } => {}
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    check_unreachable_in_expr(arg, scope, atom_names, warnings);
                }
            }
        }
    }
}

/// Check if a pattern is a catch-all (matches any input unconditionally).
/// A variable bound in `scope` is a comparison, not a catch-all.
pub(super) fn is_catch_all(pat: &Pattern, scope: &HashSet<AtomId>) -> bool {
    match pat {
        Pattern::Wildcard(_) => true,
        Pattern::Variable(id, _) => !scope.contains(id),
        Pattern::Match(p1, p2, _) => is_catch_all(p1, scope) && is_catch_all(p2, scope),
        _ => false,
    }
}

/// The exception classes a catch clause matches.
#[derive(Clone, Copy, PartialEq)]
enum Classes {
    /// `_` or a fresh variable.
    Every,
    /// `throw`, also an omitted class.
    Throw,
    Error,
    Exit,
    /// A bound name: one class, known only at run time.
    Bound,
}

fn catch_classes(
    class: &ast::CatchClass,
    scope: &HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
) -> Classes {
    match class {
        ast::CatchClass::Variable(id, _)
            if is_wildcard_name(*id, atom_names) || !scope.contains(id) =>
        {
            Classes::Every
        }
        ast::CatchClass::Variable(..) => Classes::Bound,
        ast::CatchClass::Throw => Classes::Throw,
        ast::CatchClass::Error => Classes::Error,
        ast::CatchClass::Exit => Classes::Exit,
    }
}

/// The parser represents a `_` catch class as a variable named `_`.
fn is_wildcard_name(id: AtomId, atom_names: &HashMap<AtomId, String>) -> bool {
    atom_names.get(&id).is_some_and(|name| name == "_")
}

/// Check if a set of clause patterns (e.g. function head arguments) form a
/// true catch-all.  Each individual pattern must be structurally catch-all AND
/// no variable may appear more than once across all patterns.  A repeated
/// variable (e.g. `foo(X, X)`) imposes an equality constraint and is therefore
/// not a catch-all. Head variables are always fresh, so no outer scope
/// applies.
pub(super) fn is_patterns_catch_all(patterns: &[Pattern]) -> bool {
    let fresh = HashSet::new();
    if !patterns.iter().all(|p| is_catch_all(p, &fresh)) {
        return false;
    }
    !has_repeated_pattern_vars(patterns)
}

/// Return `true` if any variable name appears more than once across all the
/// given patterns.
fn has_repeated_pattern_vars(patterns: &[Pattern]) -> bool {
    let mut ids = Vec::new();
    for pat in patterns {
        collect_pattern_var_ids(pat, &mut ids);
    }
    let mut seen = HashSet::new();
    ids.iter().any(|id| !seen.insert(*id))
}
