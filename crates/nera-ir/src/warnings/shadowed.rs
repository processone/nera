//! Shadowed variable detection.
//!
//! In Erlang, `fun` clauses and list comprehension generators introduce
//! new scopes. A pattern variable in these constructs can shadow a
//! variable from an enclosing scope.  `case`, `receive`, and `try`
//! patterns do NOT create new scopes — they extend the current scope
//! (single-assignment match semantics), so no shadow warning for those.

use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;

use super::ast_walkers::{collect_pattern_bindings, collect_pattern_bindings_with_spans};

/// Check a function for shadowed variables in nested scopes.
pub(super) fn check_shadowed_variables_fn(
    func: &ast::Function,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    for clause in &func.clauses {
        // Collect the variables bound in the function head — these form
        // the outer scope visible to any nested fun / list comprehension.
        let mut outer_scope = HashSet::new();
        for pat in &clause.patterns {
            collect_pattern_bindings(pat, &mut outer_scope);
        }
        // Body expressions can introduce new bindings via `=` matches.
        // Walk the body collecting match-bound variables into outer_scope
        // while simultaneously checking nested funs/LCs for shadows.
        check_shadowed_in_body(&clause.body, &mut outer_scope, atom_names, warnings);
    }
}

/// Walk a body (sequence of expressions), accumulating bindings from
/// match expressions into `scope` and recursing into nested constructs.
fn check_shadowed_in_body(
    body: &[ast::Expr],
    scope: &mut HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    for expr in body {
        // A top-level `Pat = Expr` in a body introduces new bindings
        // into the current scope.
        if let ast::Expr::Match(pat, rhs, _) = expr {
            check_shadowed_in_expr(rhs, scope, atom_names, warnings);
            collect_pattern_bindings(pat, scope);
        } else {
            check_shadowed_in_expr(expr, scope, atom_names, warnings);
        }
    }
}

/// Recursively walk an expression looking for `fun` clauses and list
/// comprehension generators that shadow variables from `outer_scope`.
fn check_shadowed_in_expr(
    expr: &ast::Expr,
    outer_scope: &HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    match expr {
        // ── fun clauses introduce a new scope ──────────────────────
        ast::Expr::Fun(_name, clauses, _) => {
            for clause in clauses {
                // Check each pattern var against the outer scope.
                let mut fun_scope = HashSet::new();
                for pat in &clause.patterns {
                    let mut bound = Vec::new();
                    collect_pattern_bindings_with_spans(pat, &mut bound);
                    for (atom_id, span) in &bound {
                        if outer_scope.contains(atom_id) {
                            let name = atom_names
                                .get(atom_id)
                                .map(|s| s.as_str())
                                .unwrap_or("?");
                            // Skip _-prefixed variables
                            if !name.starts_with('_') {
                                warnings.push(Diagnostic::warning_at(
                                    format!(
                                        "variable '{}' in fun clause shadows outer variable",
                                        name
                                    ),
                                    *span,
                                ).with_code("shadowed_variable"));
                            }
                        }
                        fun_scope.insert(*atom_id);
                    }
                }
                // The inner scope for the fun body is outer ∪ fun patterns.
                let mut inner_scope: HashSet<AtomId> =
                    outer_scope.union(&fun_scope).copied().collect();
                check_shadowed_in_body(&clause.body, &mut inner_scope, atom_names, warnings);
            }
        }

        // ── List/binary comprehension generators introduce a new scope ────
        ast::Expr::ListComp(body_expr, qualifiers, _)
        | ast::Expr::BinComp(body_expr, qualifiers, _) => {
            let mut lc_scope: HashSet<AtomId> = outer_scope.clone();
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(pat, source_expr, _)
                    | ast::LcQualifier::BinGenerator(pat, source_expr, _) => {
                        // The source expression is evaluated in the current scope.
                        check_shadowed_in_expr(source_expr, &lc_scope, atom_names, warnings);
                        // Generator pattern vars can shadow outer scope.
                        let mut bound = Vec::new();
                        collect_pattern_bindings_with_spans(pat, &mut bound);
                        for (atom_id, span) in &bound {
                            if outer_scope.contains(atom_id) {
                                let name = atom_names
                                    .get(atom_id)
                                    .map(|s| s.as_str())
                                    .unwrap_or("?");
                                if !name.starts_with('_') {
                                    warnings.push(Diagnostic::warning_at(
                                        format!(
                                            "variable '{}' in comprehension generator shadows outer variable",
                                            name
                                        ),
                                        *span,
                                    ).with_code("shadowed_variable"));
                                }
                            }
                            lc_scope.insert(*atom_id);
                        }
                    }
                    ast::LcQualifier::Filter(filter_expr, _) => {
                        check_shadowed_in_expr(filter_expr, &lc_scope, atom_names, warnings);
                    }
                }
            }
            check_shadowed_in_expr(body_expr, &lc_scope, atom_names, warnings);
        }

        // ── case / receive / try: patterns extend current scope, recurse into bodies ──
        ast::Expr::Case(scrutinee, clauses, _) => {
            check_shadowed_in_expr(scrutinee, outer_scope, atom_names, warnings);
            for clause in clauses {
                let mut clause_scope = outer_scope.clone();
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                check_shadowed_in_body(&clause.body, &mut clause_scope, atom_names, warnings);
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                let mut clause_scope = outer_scope.clone();
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                check_shadowed_in_body(&clause.body, &mut clause_scope, atom_names, warnings);
            }
            if let Some(after) = after {
                check_shadowed_in_expr(&after.timeout, outer_scope, atom_names, warnings);
                check_shadowed_in_body(&after.body, &mut outer_scope.clone(), atom_names, warnings);
            }
        }
        ast::Expr::Try {
            body,
            of_clauses,
            catch_clauses,
            after_body,
            ..
        } => {
            check_shadowed_in_body(body, &mut outer_scope.clone(), atom_names, warnings);
            for clause in of_clauses {
                let mut clause_scope = outer_scope.clone();
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                check_shadowed_in_body(&clause.body, &mut clause_scope, atom_names, warnings);
            }
            for clause in catch_clauses {
                let mut clause_scope = outer_scope.clone();
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                check_shadowed_in_body(&clause.body, &mut clause_scope, atom_names, warnings);
            }
            if let Some(after) = after_body {
                check_shadowed_in_body(after, &mut outer_scope.clone(), atom_names, warnings);
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                check_shadowed_in_body(&clause.body, &mut outer_scope.clone(), atom_names, warnings);
            }
        }

        // ── Recurse into sub-expressions ────────────────────────────
        ast::Expr::Block(exprs, _) | ast::Expr::Tuple(exprs, _) => {
            for e in exprs {
                check_shadowed_in_expr(e, outer_scope, atom_names, warnings);
            }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) | ast::Expr::BinOp(_, h, t, _) => {
            check_shadowed_in_expr(h, outer_scope, atom_names, warnings);
            check_shadowed_in_expr(t, outer_scope, atom_names, warnings);
        }
        ast::Expr::Match(_pat, e, _) => {
            check_shadowed_in_expr(e, outer_scope, atom_names, warnings);
            // Pattern bindings in non-body context don't extend scope
            // (handled by check_shadowed_in_body for body-level matches).
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            check_shadowed_in_expr(e, outer_scope, atom_names, warnings);
        }
        ast::Expr::Call(target, args, _) => {
            if let ast::CallTarget::Remote(module_expr, fun_expr) = target {
                check_shadowed_in_expr(module_expr, outer_scope, atom_names, warnings);
                check_shadowed_in_expr(fun_expr, outer_scope, atom_names, warnings);
            }
            for arg in args {
                check_shadowed_in_expr(arg, outer_scope, atom_names, warnings);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_shadowed_in_expr(base, outer_scope, atom_names, warnings);
            for f in fields {
                check_shadowed_in_expr(&f.key, outer_scope, atom_names, warnings);
                check_shadowed_in_expr(&f.value, outer_scope, atom_names, warnings);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            check_shadowed_in_expr(base, outer_scope, atom_names, warnings);
            for f in fields {
                check_shadowed_in_expr(&f.value, outer_scope, atom_names, warnings);
            }
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                check_shadowed_in_expr(&f.value, outer_scope, atom_names, warnings);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            check_shadowed_in_expr(base, outer_scope, atom_names, warnings);
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                check_shadowed_in_expr(&seg.value, outer_scope, atom_names, warnings);
                if let Some(ref size) = seg.size {
                    check_shadowed_in_expr(size, outer_scope, atom_names, warnings);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                check_shadowed_in_expr(&f.key, outer_scope, atom_names, warnings);
                check_shadowed_in_expr(&f.value, outer_scope, atom_names, warnings);
            }
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => {
                        check_shadowed_in_expr(e, outer_scope, atom_names, warnings);
                    }
                    ast::MaybeExpr::CondMatch(_, e, _) => {
                        check_shadowed_in_expr(e, outer_scope, atom_names, warnings);
                    }
                }
            }
            for clause in else_clauses {
                for e in &clause.body {
                    check_shadowed_in_expr(e, outer_scope, atom_names, warnings);
                }
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
        | ast::Expr::FunRef(ast::FunRefTarget::Local(..) | ast::FunRefTarget::Remote(..), _)
        | ast::Expr::RecordIndex { .. } => {}
        ast::Expr::FunRef(ast::FunRefTarget::DynamicRemote(m, f, a), _) => {
            check_shadowed_in_expr(m, outer_scope, atom_names, warnings);
            check_shadowed_in_expr(f, outer_scope, atom_names, warnings);
            check_shadowed_in_expr(a, outer_scope, atom_names, warnings);
        }
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    check_shadowed_in_expr(arg, outer_scope, atom_names, warnings);
                }
            }
        }
    }
}
