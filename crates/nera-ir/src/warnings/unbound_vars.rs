//! Unbound variable detection.
//!
//! In Erlang, referencing a variable that was never bound (assigned) in the
//! current scope is a compile error.  This pass walks the AST maintaining a
//! set of variables in scope and emits an error for each `Expr::Variable`
//! that is not in scope.

use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;

use super::ast_walkers::collect_pattern_bindings;

/// Check a function for unbound variable references.
pub(super) fn check_unbound_variables_fn(
    func: &ast::Function,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for clause in &func.clauses {
        let mut scope = HashSet::new();
        for pat in &clause.patterns {
            collect_pattern_bindings(pat, &mut scope);
        }
        if let Some(ref guard) = clause.guard {
            check_unbound_in_guard(guard, &scope, atom_names, diagnostics);
        }
        check_unbound_in_body(&clause.body, &mut scope, atom_names, diagnostics);
    }
}

/// Record defaults have no enclosing function scope, even when unused or
/// overridden at construction. Check their fun/generator inputs as well.
pub(super) fn check_unbound_variables_records(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for record in &module.records {
        for field in &record.fields {
            if let Some(default) = &field.default {
                check_unbound_in_expr(default, &HashSet::new(), atom_names, diagnostics);
            }
        }
    }
}

/// Walk a body (sequence of expressions), accumulating match bindings into
/// `scope` and checking each expression for unbound references.
fn check_unbound_in_body(
    body: &[ast::Expr],
    scope: &mut HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for expr in body {
        if let ast::Expr::Match(pat, rhs, _) = expr {
            // Check RHS first (evaluated before the binding takes effect).
            check_unbound_in_expr(rhs, scope, atom_names, diagnostics);
            // Pattern variables are bound for subsequent expressions.
            collect_pattern_bindings(pat, scope);
            // Also pull in bindings from nested Match expressions on the RHS
            // — e.g. `A = B = Expr` where `B = Expr` is the inner Match.
            collect_exported_bindings(rhs, scope);
        } else {
            check_unbound_in_expr(expr, scope, atom_names, diagnostics);
            // Nested Match expressions embedded in sub-expressions
            // (call args, tuple elements, etc.) also export bindings
            // to the enclosing body scope.
            collect_exported_bindings(expr, scope);
        }
    }
}

/// Which paths through a branching expression a binding must cover.
#[derive(Clone, Copy, PartialEq)]
enum Paths {
    /// Bound on every path: the names later code may read.
    Every,
    /// Bound on some path, including inside `try` and `maybe`.
    Any,
}

/// Collect pattern bindings from `Expr::Match` subexpressions that leak into
/// the enclosing function-clause scope. Branches export only their common
/// bindings; funs, comprehensions, try and maybe keep their local bindings.
/// The unused and unreachable checks use it to learn which names a later
/// pattern compares against.
pub(super) fn collect_exported_bindings(expr: &ast::Expr, scope: &mut HashSet<AtomId>) {
    collect_bindings(expr, scope, Paths::Every);
}

/// Collect the names `expr` binds on the given paths. Funs and comprehensions
/// keep their bindings local on every path.
fn collect_bindings(expr: &ast::Expr, scope: &mut HashSet<AtomId>, paths: Paths) {
    match expr {
        ast::Expr::Match(pat, rhs, _) => {
            collect_pattern_bindings(pat, scope);
            collect_bindings(rhs, scope, paths);
        }
        ast::Expr::Tuple(exprs, _) | ast::Expr::Block(exprs, _) => {
            for e in exprs {
                collect_bindings(e, scope, paths);
            }
        }
        ast::Expr::Cons(h, t, _)
        | ast::Expr::Send(h, t, _)
        | ast::Expr::BinOp(_, h, t, _) => {
            collect_bindings(h, scope, paths);
            collect_bindings(t, scope, paths);
        }
        ast::Expr::UnaryOp(_, e, _)
        | ast::Expr::Throw(e, _)
        | ast::Expr::Catch(e, _)
        | ast::Expr::RecordAccess { base: e, .. } => {
            collect_bindings(e, scope, paths);
        }
        ast::Expr::Call(target, args, _) => {
            if let ast::CallTarget::Remote(m, f) = target {
                collect_bindings(m, scope, paths);
                collect_bindings(f, scope, paths);
            }
            for arg in args {
                collect_bindings(arg, scope, paths);
            }
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                collect_bindings(&seg.value, scope, paths);
                if let Some(ref size) = seg.size {
                    collect_bindings(size, scope, paths);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                collect_bindings(&f.key, scope, paths);
                collect_bindings(&f.value, scope, paths);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            collect_bindings(base, scope, paths);
            for f in fields {
                collect_bindings(&f.key, scope, paths);
                collect_bindings(&f.value, scope, paths);
            }
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                collect_bindings(&f.value, scope, paths);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            collect_bindings(base, scope, paths);
            for f in fields {
                collect_bindings(&f.value, scope, paths);
            }
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            collect_bindings(scrutinee, scope, paths);
            let branches = clauses
                .iter()
                .map(|c| branch_bindings(Some(&c.pattern), &c.body, paths));
            merge_branch_bindings(branches, scope, paths);
        }
        ast::Expr::If(clauses, _) => {
            let branches = clauses.iter().map(|c| branch_bindings(None, &c.body, paths));
            merge_branch_bindings(branches, scope, paths);
        }
        ast::Expr::Receive(clauses, after, _) => {
            let branches = |paths| -> Vec<HashSet<AtomId>> {
                clauses
                    .iter()
                    .map(|c| branch_bindings(Some(&c.pattern), &c.body, paths))
                    .chain(after.iter().map(|a| branch_bindings(None, &a.body, paths)))
                    .collect()
            };
            // The timeout runs before the receive waits, so its bindings
            // hold whichever branch ends the receive. As in erlc, a name
            // that some branches may bind again, but not all, is unsafe
            // after it.
            if let Some(after) = after {
                let mut timeout = HashSet::new();
                collect_bindings(&after.timeout, &mut timeout, paths);
                if paths == Paths::Every {
                    let may_bind = branches(Paths::Any);
                    let must_bind = branches(Paths::Every);
                    timeout.retain(|name| {
                        must_bind.iter().all(|b| b.contains(name))
                            || !may_bind.iter().any(|b| b.contains(name))
                    });
                }
                scope.extend(timeout);
            }
            merge_branch_bindings(branches(paths), scope, paths);
        }
        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            if paths == Paths::Any {
                for e in body.iter().chain(after_body.iter().flatten()) {
                    collect_bindings(e, scope, paths);
                }
                for c in of_clauses {
                    scope.extend(branch_bindings(Some(&c.pattern), &c.body, paths));
                }
                for c in catch_clauses {
                    if let ast::CatchClass::Variable(name, _) = c.class {
                        scope.insert(name);
                    }
                    if let Some(stacktrace) = &c.stacktrace {
                        collect_pattern_bindings(stacktrace, scope);
                    }
                    scope.extend(branch_bindings(Some(&c.pattern), &c.body, paths));
                }
            }
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            if paths == Paths::Any {
                for item in body {
                    match item {
                        ast::MaybeExpr::Expr(e) => collect_bindings(e, scope, paths),
                        ast::MaybeExpr::CondMatch(pat, value, _) => {
                            collect_pattern_bindings(pat, scope);
                            collect_bindings(value, scope, paths);
                        }
                    }
                }
                for c in else_clauses {
                    scope.extend(branch_bindings(Some(&c.pattern), &c.body, paths));
                }
            }
        }
        // Scope-creating / no-op nodes — do not leak bindings.
        ast::Expr::Fun(_, _, _)
        | ast::Expr::ListComp(_, _, _)
        | ast::Expr::BinComp(_, _, _)
        | ast::Expr::Variable(_, _)
        | ast::Expr::Integer(_, _)
        | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(_, _)
        | ast::Expr::Atom(_, _)
        | ast::Expr::String(_, _)
        | ast::Expr::Nil(_)
        | ast::Expr::FunRef(ast::FunRefTarget::Local(..) | ast::FunRefTarget::Remote(..), _)
        | ast::Expr::RecordIndex { .. }
        | ast::Expr::MacroCall { .. } => {}
        ast::Expr::FunRef(ast::FunRefTarget::DynamicRemote(m, f, a), _) => {
            collect_bindings(m, scope, paths);
            collect_bindings(f, scope, paths);
            collect_bindings(a, scope, paths);
        }
    }
}

/// The names one branch binds on the given paths: its pattern's and its
/// body's.
fn branch_bindings(
    pattern: Option<&ast::Pattern>,
    body: &[ast::Expr],
    paths: Paths,
) -> HashSet<AtomId> {
    let mut bound = HashSet::new();
    if let Some(pattern) = pattern {
        collect_pattern_bindings(pattern, &mut bound);
    }
    for expr in body {
        collect_bindings(expr, &mut bound, paths);
    }
    bound
}

/// Add the names the branches bind on the given paths. On every path, a
/// key/size after a branch can read names bound on every successful path.
fn merge_branch_bindings(
    branches: impl IntoIterator<Item = HashSet<AtomId>>,
    scope: &mut HashSet<AtomId>,
    paths: Paths,
) {
    let mut bindings = branches.into_iter();
    if paths == Paths::Any {
        bindings.for_each(|branch| scope.extend(branch));
    } else if let Some(mut common) = bindings.next() {
        for branch in bindings {
            common.retain(|name| branch.contains(name));
        }
        scope.extend(common);
    }
}

/// Check a single expression for unbound variable references.
fn check_unbound_in_expr(
    expr: &ast::Expr,
    scope: &HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match expr {
        ast::Expr::Variable(id, span) => {
            if !scope.contains(id) {
                if let Some(name) = atom_names.get(id).map(|s| s.as_str()) {
                    // Skip underscore — bare `_` is never a real variable reference.
                    if name != "_" {
                        diagnostics.push(Diagnostic::error_at(
                            format!("variable '{}' is unbound", name),
                            *span,
                        ));
                    }
                }
            }
        }

        // ── fun: new scope from parameters, captures outer scope ──
        ast::Expr::Fun(name, clauses, _) => {
            for clause in clauses {
                let mut fun_scope: HashSet<AtomId> = scope.clone();
                // The named fun's own name is in scope throughout each
                // clause, including its pattern inputs.
                if let Some(fun_name) = name {
                    fun_scope.insert(*fun_name);
                }
                // Keys/sizes read the enclosing inputs, not bindings from
                // this head (including sibling arguments). The named fun's
                // own name is already available, as it is in guards/bodies.
                for pat in &clause.patterns {
                    check_unbound_in_pattern_exprs(pat, &fun_scope, atom_names, diagnostics);
                }
                for pat in &clause.patterns {
                    collect_pattern_bindings(pat, &mut fun_scope);
                }
                if let Some(ref guard) = clause.guard {
                    check_unbound_in_guard(guard, &fun_scope, atom_names, diagnostics);
                }
                check_unbound_in_body(&clause.body, &mut fun_scope, atom_names, diagnostics);
            }
        }

        // ── case ──
        ast::Expr::Case(scrutinee, clauses, _) => {
            check_unbound_in_expr(scrutinee, scope, atom_names, diagnostics);
            let mut input_scope = scope.clone();
            collect_exported_bindings(scrutinee, &mut input_scope);
            for clause in clauses {
                let mut clause_scope = input_scope.clone();
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                if let Some(ref guard) = clause.guard {
                    check_unbound_in_guard(guard, &clause_scope, atom_names, diagnostics);
                }
                check_unbound_in_body(&clause.body, &mut clause_scope, atom_names, diagnostics);
            }
        }

        // ── if: no patterns, guards evaluated in current scope ──
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                check_unbound_in_guard(&clause.guard, scope, atom_names, diagnostics);
                check_unbound_in_body(&clause.body, &mut scope.clone(), atom_names, diagnostics);
            }
        }

        // ── receive ──
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                let mut clause_scope = scope.clone();
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                if let Some(ref guard) = clause.guard {
                    check_unbound_in_guard(guard, &clause_scope, atom_names, diagnostics);
                }
                check_unbound_in_body(&clause.body, &mut clause_scope, atom_names, diagnostics);
            }
            if let Some(after) = after {
                check_unbound_in_expr(&after.timeout, scope, atom_names, diagnostics);
                check_unbound_in_body(&after.body, &mut scope.clone(), atom_names, diagnostics);
            }
        }

        // ── try ──
        ast::Expr::Try {
            body,
            of_clauses,
            catch_clauses,
            after_body,
            ..
        } => {
            // Try body: bindings flow to of-clauses.
            let mut try_scope = scope.clone();
            check_unbound_in_body(body, &mut try_scope, atom_names, diagnostics);
            for clause in of_clauses {
                let mut clause_scope = try_scope.clone();
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                if let Some(ref guard) = clause.guard {
                    check_unbound_in_guard(guard, &clause_scope, atom_names, diagnostics);
                }
                check_unbound_in_body(&clause.body, &mut clause_scope, atom_names, diagnostics);
            }
            // Catch clauses get outer scope (not try body bindings).
            for clause in catch_clauses {
                let mut clause_scope = scope.clone();
                // Class variable binding (e.g. `Class:Reason:Stacktrace`).
                if let ast::CatchClass::Variable(class_name, _) = &clause.class {
                    clause_scope.insert(*class_name);
                }
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                if let Some(ref stk) = clause.stacktrace {
                    collect_pattern_bindings(stk, &mut clause_scope);
                }
                if let Some(ref guard) = clause.guard {
                    check_unbound_in_guard(guard, &clause_scope, atom_names, diagnostics);
                }
                check_unbound_in_body(&clause.body, &mut clause_scope, atom_names, diagnostics);
            }
            // After block gets outer scope.
            if let Some(after) = after_body {
                check_unbound_in_body(after, &mut scope.clone(), atom_names, diagnostics);
            }
        }

        // ── List/binary comprehension ──
        ast::Expr::ListComp(body_expr, qualifiers, _)
        | ast::Expr::BinComp(body_expr, qualifiers, _) => {
            let mut lc_scope = scope.clone();
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(pat, source_expr, _)
                    | ast::LcQualifier::BinGenerator(pat, source_expr, _) => {
                        check_unbound_in_expr(source_expr, &lc_scope, atom_names, diagnostics);
                        // Inputs can come from outer/earlier generators, but
                        // not from the bindings this generator introduces.
                        check_unbound_in_pattern_exprs(pat, &lc_scope, atom_names, diagnostics);
                        collect_pattern_bindings(pat, &mut lc_scope);
                    }
                    ast::LcQualifier::Filter(filter_expr, _) => {
                        check_unbound_in_expr(filter_expr, &lc_scope, atom_names, diagnostics);
                    }
                }
            }
            check_unbound_in_expr(body_expr, &lc_scope, atom_names, diagnostics);
        }

        // ── maybe ──
        ast::Expr::Maybe { body, else_clauses, .. } => {
            let mut maybe_scope = scope.clone();
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => {
                        if let ast::Expr::Match(pat, rhs, _) = e {
                            check_unbound_in_expr(rhs, &maybe_scope, atom_names, diagnostics);
                            collect_pattern_bindings(pat, &mut maybe_scope);
                        } else {
                            check_unbound_in_expr(e, &maybe_scope, atom_names, diagnostics);
                        }
                    }
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        check_unbound_in_expr(e, &maybe_scope, atom_names, diagnostics);
                        collect_pattern_bindings(pat, &mut maybe_scope);
                    }
                }
            }
            for clause in else_clauses {
                let mut clause_scope = scope.clone();
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                if let Some(ref guard) = clause.guard {
                    check_unbound_in_guard(guard, &clause_scope, atom_names, diagnostics);
                }
                check_unbound_in_body(&clause.body, &mut clause_scope, atom_names, diagnostics);
            }
        }

        // ── Match expression (non-body level) ──
        ast::Expr::Match(pat, rhs, _) => {
            check_unbound_in_expr(rhs, scope, atom_names, diagnostics);
            // Check pattern variables that are already bound (used as equality checks)
            // but don't emit unbound errors for pattern bindings themselves.
            check_unbound_in_pattern_exprs(pat, scope, atom_names, diagnostics);
        }

        // ── Recurse into sub-expressions ──
        ast::Expr::Block(exprs, _) => {
            check_unbound_in_body(exprs, &mut scope.clone(), atom_names, diagnostics);
        }
        ast::Expr::Tuple(exprs, _) => {
            for e in exprs {
                check_unbound_in_expr(e, scope, atom_names, diagnostics);
            }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) | ast::Expr::BinOp(_, h, t, _) => {
            check_unbound_in_expr(h, scope, atom_names, diagnostics);
            check_unbound_in_expr(t, scope, atom_names, diagnostics);
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            check_unbound_in_expr(e, scope, atom_names, diagnostics);
        }
        ast::Expr::Call(target, args, _) => {
            match target {
                ast::CallTarget::Local(name_id, span) => {
                    // Local function calls by name are not variable references.
                    // But CallTarget::Variable is a variable used as a function.
                    let _ = (name_id, span);
                }
                ast::CallTarget::Variable(id, span) => {
                    if !scope.contains(id) {
                        if let Some(name) = atom_names.get(id).map(|s| s.as_str()) {
                            if name != "_" {
                                diagnostics.push(Diagnostic::error_at(
                                    format!("variable '{}' is unbound", name),
                                    *span,
                                ));
                            }
                        }
                    }
                }
                ast::CallTarget::Remote(module_expr, fun_expr) => {
                    check_unbound_in_expr(module_expr, scope, atom_names, diagnostics);
                    check_unbound_in_expr(fun_expr, scope, atom_names, diagnostics);
                }
                ast::CallTarget::Expr(callee) => {
                    check_unbound_in_expr(callee, scope, atom_names, diagnostics);
                }
            }
            for arg in args {
                check_unbound_in_expr(arg, scope, atom_names, diagnostics);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_unbound_in_expr(base, scope, atom_names, diagnostics);
            for f in fields {
                check_unbound_in_expr(&f.key, scope, atom_names, diagnostics);
                check_unbound_in_expr(&f.value, scope, atom_names, diagnostics);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            check_unbound_in_expr(base, scope, atom_names, diagnostics);
            for f in fields {
                check_unbound_in_expr(&f.value, scope, atom_names, diagnostics);
            }
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                check_unbound_in_expr(&f.value, scope, atom_names, diagnostics);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            check_unbound_in_expr(base, scope, atom_names, diagnostics);
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                check_unbound_in_expr(&seg.value, scope, atom_names, diagnostics);
                if let Some(ref size) = seg.size {
                    check_unbound_in_expr(size, scope, atom_names, diagnostics);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                check_unbound_in_expr(&f.key, scope, atom_names, diagnostics);
                check_unbound_in_expr(&f.value, scope, atom_names, diagnostics);
            }
        }

        // Leaf nodes — no variable references.
        ast::Expr::Integer(_, _)
        | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(_, _)
        | ast::Expr::Atom(_, _)
        | ast::Expr::String(_, _)
        | ast::Expr::Nil(_)
        | ast::Expr::FunRef(ast::FunRefTarget::Local(..) | ast::FunRefTarget::Remote(..), _)
        | ast::Expr::RecordIndex { .. } => {}
        ast::Expr::FunRef(ast::FunRefTarget::DynamicRemote(m, f, a), _) => {
            check_unbound_in_expr(m, scope, atom_names, diagnostics);
            check_unbound_in_expr(f, scope, atom_names, diagnostics);
            check_unbound_in_expr(a, scope, atom_names, diagnostics);
        }
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    check_unbound_in_expr(arg, scope, atom_names, diagnostics);
                }
            }
        }
    }
}

/// Check guard expressions for unbound variable references.
fn check_unbound_in_guard(
    guard: &ast::Guard,
    scope: &HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match guard {
        ast::Guard::Test(e) => check_unbound_in_expr(e, scope, atom_names, diagnostics),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                check_unbound_in_expr(e, scope, atom_names, diagnostics);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                check_unbound_in_guard(g, scope, atom_names, diagnostics);
            }
        }
    }
}

/// Check for unbound variables used in pattern sub-expressions (e.g. binary
/// segment sizes). Pattern *bindings* themselves don't need to be checked.
fn check_unbound_in_pattern_exprs(
    pat: &ast::Pattern,
    scope: &HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match pat {
        ast::Pattern::Binary(segments, _) => {
            // Only earlier segments of this binary supplement the inputs to
            // a size. Do not export them to sibling patterns or binaries.
            let mut size_scope = scope.clone();
            for seg in segments {
                if let Some(ref size) = seg.size {
                    check_unbound_in_expr(size, &size_scope, atom_names, diagnostics);
                }
                check_unbound_in_pattern_exprs(&seg.value, scope, atom_names, diagnostics);
                collect_pattern_bindings(&seg.value, &mut size_scope);
            }
        }
        ast::Pattern::Tuple(pats, _) => {
            for p in pats {
                check_unbound_in_pattern_exprs(p, scope, atom_names, diagnostics);
            }
        }
        ast::Pattern::Cons(h, t, _) => {
            check_unbound_in_pattern_exprs(h, scope, atom_names, diagnostics);
            check_unbound_in_pattern_exprs(t, scope, atom_names, diagnostics);
        }
        ast::Pattern::Match(p1, p2, _) => {
            check_unbound_in_pattern_exprs(p1, scope, atom_names, diagnostics);
            check_unbound_in_pattern_exprs(p2, scope, atom_names, diagnostics);
        }
        ast::Pattern::Map(fields, _) => {
            for field in fields {
                // Map pattern keys are expressions that can reference variables.
                check_unbound_in_expr(&field.key, scope, atom_names, diagnostics);
                check_unbound_in_pattern_exprs(&field.value, scope, atom_names, diagnostics);
            }
        }
        ast::Pattern::Record { fields, .. } => {
            for field in fields {
                check_unbound_in_pattern_exprs(&field.pattern, scope, atom_names, diagnostics);
            }
        }
        ast::Pattern::Concat(lhs, rhs, _) => {
            check_unbound_in_pattern_exprs(lhs, scope, atom_names, diagnostics);
            check_unbound_in_pattern_exprs(rhs, scope, atom_names, diagnostics);
        }
        // Leaf patterns — nothing to check.
        ast::Pattern::Variable(_, _)
        | ast::Pattern::Wildcard(_)
        | ast::Pattern::Integer(_, _)
        | ast::Pattern::BigInteger(_, _)
        | ast::Pattern::Float(_, _)
        | ast::Pattern::Atom(_, _)
        | ast::Pattern::String(_, _)
        | ast::Pattern::Nil(_) => {}
        ast::Pattern::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    check_unbound_in_pattern_exprs(arg, scope, atom_names, diagnostics);
                }
            }
        }
    }
}
