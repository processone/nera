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

/// Funs and generator sources introduce ordinary local scopes inside a default;
/// bindings elsewhere still belong to the record definition, which forbids them.
#[derive(Clone, Copy, PartialEq)]
enum Context {
    Expression,
    RecordDefault,
}

fn variable_error(
    id: AtomId,
    span: nera_syntax::span::Span,
    context: Context,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(name) = atom_names.get(&id) {
        let message = match context {
            Context::RecordDefault => format!("variable '{name}' in record definition"),
            Context::Expression => format!("variable '{name}' is unbound"),
        };
        diagnostics.push(Diagnostic::error_at(message, span));
    }
}

/// Check a function for unbound variable references.
pub(super) fn check_unbound_variables_fn(
    func: &ast::Function,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for clause in &func.clauses {
        let mut scope = HashSet::new();
        // No argument in this head supplies another argument's key/size.
        for pat in &clause.patterns {
            check_unbound_in_pattern_exprs(pat, &scope, Context::Expression, atom_names, diagnostics);
        }
        for pat in &clause.patterns {
            collect_pattern_bindings(pat, &mut scope);
        }
        if let Some(ref guard) = clause.guard {
            check_unbound_in_guard(guard, &scope, Context::Expression, atom_names, diagnostics);
        }
        check_unbound_in_body(&clause.body, &mut scope, Context::Expression, atom_names, diagnostics);
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
                check_unbound_in_expr(default, &HashSet::new(), Context::RecordDefault, atom_names, diagnostics);
            }
        }
    }
}

/// Walk a body (sequence of expressions), accumulating match bindings into
/// `scope` and checking each expression for unbound references.
fn check_unbound_in_body(
    body: &[ast::Expr],
    scope: &mut HashSet<AtomId>,
    context: Context,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for expr in body {
        check_unbound_in_expr(expr, scope, context, atom_names, diagnostics);
        // Matches, including those nested in sub-expressions, export their
        // bindings only after their pattern inputs have been checked.
        collect_exported_bindings(expr, scope);
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
/// bindings; funs, comprehensions, try, maybe, catch operands and skipped
/// short-circuit operands do not supply definite bindings. The variable
/// diagnostics and unreachable check share this view of later pattern inputs.
pub(crate) fn collect_exported_bindings(expr: &ast::Expr, scope: &mut HashSet<AtomId>) {
    collect_bindings(expr, scope, Paths::Every);
}

/// Collect bindings even from paths that may be skipped. This is not a scope
/// of safe inputs: unsafe-variable analysis uses it to find conditional names.
/// Funs and comprehensions still keep their bindings local.
pub(super) fn collect_possible_bindings(expr: &ast::Expr, bindings: &mut HashSet<AtomId>) {
    collect_bindings(expr, bindings, Paths::Any);
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
        ast::Expr::BinOp(ast::BinOp::Andalso | ast::BinOp::Orelse, lhs, rhs, _) => {
            collect_bindings(lhs, scope, paths);
            // The right operand may not run, even on successful completion.
            if paths == Paths::Any {
                collect_bindings(rhs, scope, paths);
            }
        }
        ast::Expr::Catch(expr, _) => {
            // An exception may skip its bindings and still return a value.
            if paths == Paths::Any {
                collect_bindings(expr, scope, paths);
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
    context: Context,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match expr {
        ast::Expr::Variable(id, span) => {
            if !scope.contains(id) {
                if let Some(name) = atom_names.get(id).map(|s| s.as_str()) {
                    // Ordinary reference analysis skips `_`; a record default
                    // must still reject it when used as an expression.
                    if name != "_" || context == Context::RecordDefault {
                        variable_error(*id, *span, context, atom_names, diagnostics);
                    }
                }
            }
        }

        // ── fun: new scope from parameters, captures outer scope ──
        ast::Expr::Fun(name, clauses, _) => {
            let context = Context::Expression;
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
                    check_unbound_in_pattern_exprs(pat, &fun_scope, context, atom_names, diagnostics);
                }
                for pat in &clause.patterns {
                    collect_pattern_bindings(pat, &mut fun_scope);
                }
                if let Some(ref guard) = clause.guard {
                    check_unbound_in_guard(guard, &fun_scope, context, atom_names, diagnostics);
                }
                check_unbound_in_body(&clause.body, &mut fun_scope, context, atom_names, diagnostics);
            }
        }

        // ── case ──
        ast::Expr::Case(scrutinee, clauses, _) => {
            check_unbound_in_expr(scrutinee, scope, context, atom_names, diagnostics);
            let mut input_scope = scope.clone();
            collect_exported_bindings(scrutinee, &mut input_scope);
            for clause in clauses {
                let mut clause_scope = input_scope.clone();
                check_unbound_in_pattern_exprs(&clause.pattern, &input_scope, context, atom_names, diagnostics);
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                if let Some(ref guard) = clause.guard {
                    check_unbound_in_guard(guard, &clause_scope, context, atom_names, diagnostics);
                }
                check_unbound_in_body(&clause.body, &mut clause_scope, context, atom_names, diagnostics);
            }
        }

        // ── if: no patterns, guards evaluated in current scope ──
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                check_unbound_in_guard(&clause.guard, scope, context, atom_names, diagnostics);
                check_unbound_in_body(&clause.body, &mut scope.clone(), context, atom_names, diagnostics);
            }
        }

        // ── receive ──
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                let mut clause_scope = scope.clone();
                check_unbound_in_pattern_exprs(&clause.pattern, scope, context, atom_names, diagnostics);
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                if let Some(ref guard) = clause.guard {
                    check_unbound_in_guard(guard, &clause_scope, context, atom_names, diagnostics);
                }
                check_unbound_in_body(&clause.body, &mut clause_scope, context, atom_names, diagnostics);
            }
            if let Some(after) = after {
                check_unbound_in_expr(&after.timeout, scope, context, atom_names, diagnostics);
                check_unbound_in_body(&after.body, &mut scope.clone(), context, atom_names, diagnostics);
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
            check_unbound_in_body(body, &mut try_scope, context, atom_names, diagnostics);
            for clause in of_clauses {
                let mut clause_scope = try_scope.clone();
                check_unbound_in_pattern_exprs(&clause.pattern, &try_scope, context, atom_names, diagnostics);
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                if let Some(ref guard) = clause.guard {
                    check_unbound_in_guard(guard, &clause_scope, context, atom_names, diagnostics);
                }
                check_unbound_in_body(&clause.body, &mut clause_scope, context, atom_names, diagnostics);
            }
            // Catch clauses get outer scope (not try body bindings).
            for clause in catch_clauses {
                let mut clause_scope = scope.clone();
                // Class, reason and stacktrace bindings are all siblings;
                // none supplies an input to another pattern in this head.
                check_unbound_in_pattern_exprs(&clause.pattern, scope, context, atom_names, diagnostics);
                if let Some(stk) = &clause.stacktrace {
                    check_unbound_in_pattern_exprs(stk, scope, context, atom_names, diagnostics);
                }
                // Class variable binding (e.g. `Class:Reason:Stacktrace`).
                if let ast::CatchClass::Variable(class_name, span) = &clause.class {
                    // The parser also uses this variant for the class wildcard,
                    // which introduces no binding, just like reason/stack `_`.
                    if atom_names.get(class_name).is_some_and(|name| name != "_") {
                        if context == Context::RecordDefault && !scope.contains(class_name) {
                            variable_error(*class_name, *span, context, atom_names, diagnostics);
                        }
                        clause_scope.insert(*class_name);
                    }
                }
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                if let Some(ref stk) = clause.stacktrace {
                    collect_pattern_bindings(stk, &mut clause_scope);
                }
                if let Some(ref guard) = clause.guard {
                    check_unbound_in_guard(guard, &clause_scope, context, atom_names, diagnostics);
                }
                check_unbound_in_body(&clause.body, &mut clause_scope, context, atom_names, diagnostics);
            }
            // After block gets outer scope.
            if let Some(after) = after_body {
                check_unbound_in_body(after, &mut scope.clone(), context, atom_names, diagnostics);
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
                        // Like erlc, generator sources and patterns allow local
                        // bindings; filters and the body keep the record context.
                        check_unbound_in_expr(source_expr, &lc_scope, Context::Expression, atom_names, diagnostics);
                        // Inputs can come from outer/earlier generators, but
                        // not from the bindings this generator introduces.
                        check_unbound_in_pattern_exprs(pat, &lc_scope, Context::Expression, atom_names, diagnostics);
                        collect_pattern_bindings(pat, &mut lc_scope);
                    }
                    ast::LcQualifier::Filter(filter_expr, _) => {
                        check_unbound_in_expr(filter_expr, &lc_scope, context, atom_names, diagnostics);
                        // Every completed filter supplies definite bindings
                        // to the remaining qualifiers and template.
                        collect_exported_bindings(filter_expr, &mut lc_scope);
                    }
                }
            }
            check_unbound_in_expr(body_expr, &lc_scope, context, atom_names, diagnostics);
        }

        // ── maybe ──
        ast::Expr::Maybe { body, else_clauses, .. } => {
            let mut maybe_scope = scope.clone();
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => {
                        check_unbound_in_expr(e, &maybe_scope, context, atom_names, diagnostics);
                        collect_exported_bindings(e, &mut maybe_scope);
                    }
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        check_unbound_in_expr(e, &maybe_scope, context, atom_names, diagnostics);
                        collect_exported_bindings(e, &mut maybe_scope);
                        check_unbound_in_pattern_exprs(pat, &maybe_scope, context, atom_names, diagnostics);
                        collect_pattern_bindings(pat, &mut maybe_scope);
                    }
                }
            }
            for clause in else_clauses {
                let mut clause_scope = scope.clone();
                check_unbound_in_pattern_exprs(&clause.pattern, scope, context, atom_names, diagnostics);
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                if let Some(ref guard) = clause.guard {
                    check_unbound_in_guard(guard, &clause_scope, context, atom_names, diagnostics);
                }
                check_unbound_in_body(&clause.body, &mut clause_scope, context, atom_names, diagnostics);
            }
        }

        // ── Match expression ──
        ast::Expr::Match(pat, rhs, _) => {
            check_unbound_in_expr(rhs, scope, context, atom_names, diagnostics);
            // The RHS completes before matching, so its definite bindings
            // are inputs; bindings introduced by this pattern are not.
            let mut input_scope = scope.clone();
            collect_exported_bindings(rhs, &mut input_scope);
            check_unbound_in_pattern_exprs(pat, &input_scope, context, atom_names, diagnostics);
        }

        // ── Recurse into sub-expressions ──
        ast::Expr::Block(exprs, _) => {
            check_unbound_in_body(exprs, &mut scope.clone(), context, atom_names, diagnostics);
        }
        ast::Expr::Tuple(exprs, _) => {
            for e in exprs {
                check_unbound_in_expr(e, scope, context, atom_names, diagnostics);
            }
        }
        ast::Expr::BinOp(ast::BinOp::Andalso | ast::BinOp::Orelse, lhs, rhs, _) => {
            check_unbound_in_expr(lhs, scope, atom_names, diagnostics);
            let mut input_scope = scope.clone();
            collect_exported_bindings(lhs, &mut input_scope);
            check_unbound_in_expr(rhs, &input_scope, atom_names, diagnostics);
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) | ast::Expr::BinOp(_, h, t, _) => {
            check_unbound_in_expr(h, scope, context, atom_names, diagnostics);
            check_unbound_in_expr(t, scope, context, atom_names, diagnostics);
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            check_unbound_in_expr(e, scope, context, atom_names, diagnostics);
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
                            if name != "_" || context == Context::RecordDefault {
                                variable_error(*id, *span, context, atom_names, diagnostics);
                            }
                        }
                    }
                }
                ast::CallTarget::Remote(module_expr, fun_expr) => {
                    check_unbound_in_expr(module_expr, scope, context, atom_names, diagnostics);
                    check_unbound_in_expr(fun_expr, scope, context, atom_names, diagnostics);
                }
                ast::CallTarget::Expr(callee) => {
                    check_unbound_in_expr(callee, scope, context, atom_names, diagnostics);
                }
            }
            for arg in args {
                check_unbound_in_expr(arg, scope, context, atom_names, diagnostics);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_unbound_in_expr(base, scope, context, atom_names, diagnostics);
            for f in fields {
                check_unbound_in_expr(&f.key, scope, context, atom_names, diagnostics);
                check_unbound_in_expr(&f.value, scope, context, atom_names, diagnostics);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            check_unbound_in_expr(base, scope, context, atom_names, diagnostics);
            for f in fields {
                check_unbound_in_expr(&f.value, scope, context, atom_names, diagnostics);
            }
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                check_unbound_in_expr(&f.value, scope, context, atom_names, diagnostics);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            check_unbound_in_expr(base, scope, context, atom_names, diagnostics);
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                check_unbound_in_expr(&seg.value, scope, context, atom_names, diagnostics);
                if let Some(ref size) = seg.size {
                    check_unbound_in_expr(size, scope, context, atom_names, diagnostics);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                check_unbound_in_expr(&f.key, scope, context, atom_names, diagnostics);
                check_unbound_in_expr(&f.value, scope, context, atom_names, diagnostics);
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
            check_unbound_in_expr(m, scope, context, atom_names, diagnostics);
            check_unbound_in_expr(f, scope, context, atom_names, diagnostics);
            check_unbound_in_expr(a, scope, context, atom_names, diagnostics);
        }
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    check_unbound_in_expr(arg, scope, context, atom_names, diagnostics);
                }
            }
        }
    }
}

/// Check guard expressions for unbound variable references.
fn check_unbound_in_guard(
    guard: &ast::Guard,
    scope: &HashSet<AtomId>,
    context: Context,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match guard {
        ast::Guard::Test(e) => check_unbound_in_expr(e, scope, context, atom_names, diagnostics),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                check_unbound_in_expr(e, scope, context, atom_names, diagnostics);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                check_unbound_in_guard(g, scope, context, atom_names, diagnostics);
            }
        }
    }
}

/// Check for unbound variables used in pattern sub-expressions (e.g. binary
/// segment sizes). In a record default, new pattern bindings are errors too.
fn check_unbound_in_pattern_exprs(
    pat: &ast::Pattern,
    scope: &HashSet<AtomId>,
    context: Context,
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
                    check_unbound_in_expr(size, &size_scope, context, atom_names, diagnostics);
                }
                check_unbound_in_pattern_exprs(&seg.value, scope, context, atom_names, diagnostics);
                collect_pattern_bindings(&seg.value, &mut size_scope);
            }
        }
        ast::Pattern::Tuple(pats, _) => {
            for p in pats {
                check_unbound_in_pattern_exprs(p, scope, context, atom_names, diagnostics);
            }
        }
        ast::Pattern::Cons(h, t, _) => {
            check_unbound_in_pattern_exprs(h, scope, context, atom_names, diagnostics);
            check_unbound_in_pattern_exprs(t, scope, context, atom_names, diagnostics);
        }
        ast::Pattern::Match(p1, p2, _) => {
            check_unbound_in_pattern_exprs(p1, scope, context, atom_names, diagnostics);
            check_unbound_in_pattern_exprs(p2, scope, context, atom_names, diagnostics);
        }
        ast::Pattern::Map(fields, _) => {
            for field in fields {
                // Map pattern keys are expressions that can reference variables.
                check_unbound_in_expr(&field.key, scope, context, atom_names, diagnostics);
                check_unbound_in_pattern_exprs(&field.value, scope, context, atom_names, diagnostics);
            }
        }
        ast::Pattern::Record { fields, .. } => {
            for field in fields {
                check_unbound_in_pattern_exprs(&field.pattern, scope, context, atom_names, diagnostics);
            }
        }
        ast::Pattern::Concat(lhs, rhs, _) => {
            check_unbound_in_pattern_exprs(lhs, scope, context, atom_names, diagnostics);
            check_unbound_in_pattern_exprs(rhs, scope, context, atom_names, diagnostics);
        }
        ast::Pattern::Variable(id, span) => {
            if context == Context::RecordDefault && !scope.contains(id) {
                variable_error(*id, *span, context, atom_names, diagnostics);
            }
        }
        // Literal and wildcard patterns introduce no variables.
        ast::Pattern::Wildcard(_)
        | ast::Pattern::Integer(_, _)
        | ast::Pattern::BigInteger(_, _)
        | ast::Pattern::Float(_, _)
        | ast::Pattern::Atom(_, _)
        | ast::Pattern::String(_, _)
        | ast::Pattern::Nil(_) => {}
        ast::Pattern::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    check_unbound_in_pattern_exprs(arg, scope, context, atom_names, diagnostics);
                }
            }
        }
    }
}
