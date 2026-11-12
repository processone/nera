//! Warn when a `_`-prefixed variable appears in a pattern position but is
//! already bound in scope.  OTP emits `{match_underscore_var, V}` for this.
//!
//! The programmer likely meant bare `_` (don't care) rather than an equality
//! check against the existing value.

use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;
use nera_syntax::span::Span;

use super::ast_walkers::{collect_pattern_bindings, collect_pattern_bindings_with_spans};

/// Check a function for `_`-prefixed variables reused in pattern position.
pub(super) fn check_match_underscore_vars_fn(
    func: &ast::Function,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    for clause in &func.clauses {
        // Each function clause starts with a fresh scope.
        let mut scope = HashSet::new();
        // Walk clause head patterns left-to-right, building scope incrementally.
        for pat in &clause.patterns {
            check_pattern_for_underscore_reuse(pat, &scope, atom_names, warnings);
            collect_pattern_bindings(pat, &mut scope);
        }
        // Walk clause body with accumulated scope.
        check_body(&clause.body, &mut scope, atom_names, warnings);
    }
}

/// Check a single pattern for `_`-prefixed variables that are already in scope.
fn check_pattern_for_underscore_reuse(
    pat: &ast::Pattern,
    scope: &HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    let mut bound = Vec::new();
    collect_pattern_bindings_with_spans(pat, &mut bound);
    for (atom_id, span) in &bound {
        if scope.contains(atom_id) {
            if let Some(name) = atom_names.get(atom_id) {
                if name.starts_with('_') && name.len() > 1 {
                    emit_warning(name, *span, warnings);
                }
            }
        }
    }
}

/// Walk a body sequentially, accumulating scope from match expressions
/// and checking patterns for underscore variable reuse.
fn check_body(
    body: &[ast::Expr],
    scope: &mut HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    for expr in body {
        if let ast::Expr::Match(pat, rhs, _) = expr {
            // Check the pattern against current scope.
            check_pattern_for_underscore_reuse(pat, scope, atom_names, warnings);
            // Recurse into the RHS expression.
            check_expr(rhs, scope, atom_names, warnings);
            // Add bindings from this match to scope for subsequent expressions.
            collect_pattern_bindings(pat, scope);
            // Handle chained match RHS bindings (X = Y = 1).
            collect_chained_match_bindings(rhs, scope);
        } else {
            check_expr(expr, scope, atom_names, warnings);
            // A top-level match in the body may also introduce bindings
            // (handled above). No new bindings from non-match expressions.
        }
    }
}

/// Collect bindings from chained match expressions (e.g., `Y = 1` in `X = Y = 1`).
fn collect_chained_match_bindings(expr: &ast::Expr, scope: &mut HashSet<AtomId>) {
    if let ast::Expr::Match(pat, rhs, _) = expr {
        collect_pattern_bindings(pat, scope);
        collect_chained_match_bindings(rhs, scope);
    }
}

/// Recursively walk an expression looking for nested clause constructs.
fn check_expr(
    expr: &ast::Expr,
    scope: &HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    match expr {
        ast::Expr::Case(scrutinee, clauses, _) => {
            check_expr(scrutinee, scope, atom_names, warnings);
            for clause in clauses {
                check_clause_patterns_and_body(
                    std::slice::from_ref(&clause.pattern),
                    &clause.body,
                    scope,
                    atom_names,
                    warnings,
                );
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                check_clause_patterns_and_body(
                    std::slice::from_ref(&clause.pattern),
                    &clause.body,
                    scope,
                    atom_names,
                    warnings,
                );
            }
            if let Some(after) = after {
                check_expr(&after.timeout, scope, atom_names, warnings);
                check_body(&after.body, &mut scope.clone(), atom_names, warnings);
            }
        }
        ast::Expr::Try {
            body,
            of_clauses,
            catch_clauses,
            after_body,
            ..
        } => {
            check_body(body, &mut scope.clone(), atom_names, warnings);
            for clause in of_clauses {
                check_clause_patterns_and_body(
                    std::slice::from_ref(&clause.pattern),
                    &clause.body,
                    scope,
                    atom_names,
                    warnings,
                );
            }
            for clause in catch_clauses {
                check_clause_patterns_and_body(
                    std::slice::from_ref(&clause.pattern),
                    &clause.body,
                    scope,
                    atom_names,
                    warnings,
                );
            }
            if let Some(after) = after_body {
                check_body(after, &mut scope.clone(), atom_names, warnings);
            }
        }
        ast::Expr::Fun(_name, clauses, _) => {
            // Fun starts a fresh scope — don't propagate outer bindings.
            for clause in clauses {
                let mut fun_scope = HashSet::new();
                for pat in &clause.patterns {
                    check_pattern_for_underscore_reuse(pat, &fun_scope, atom_names, warnings);
                    collect_pattern_bindings(pat, &mut fun_scope);
                }
                check_body(&clause.body, &mut fun_scope, atom_names, warnings);
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                check_body(&clause.body, &mut scope.clone(), atom_names, warnings);
            }
        }
        ast::Expr::Block(exprs, _) => {
            check_body(exprs, &mut scope.clone(), atom_names, warnings);
        }
        ast::Expr::Tuple(exprs, _) => {
            for e in exprs {
                check_expr(e, scope, atom_names, warnings);
            }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) | ast::Expr::BinOp(_, h, t, _) => {
            check_expr(h, scope, atom_names, warnings);
            check_expr(t, scope, atom_names, warnings);
        }
        ast::Expr::Match(pat, e, _) => {
            // Non-body-level match (e.g., inside a tuple or call arg).
            check_pattern_for_underscore_reuse(pat, scope, atom_names, warnings);
            check_expr(e, scope, atom_names, warnings);
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            check_expr(e, scope, atom_names, warnings);
        }
        ast::Expr::Call(target, args, _) => {
            if let ast::CallTarget::Remote(module_expr, fun_expr) = target {
                check_expr(module_expr, scope, atom_names, warnings);
                check_expr(fun_expr, scope, atom_names, warnings);
            }
            for arg in args {
                check_expr(arg, scope, atom_names, warnings);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_expr(base, scope, atom_names, warnings);
            for f in fields {
                check_expr(&f.key, scope, atom_names, warnings);
                check_expr(&f.value, scope, atom_names, warnings);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            check_expr(base, scope, atom_names, warnings);
            for f in fields {
                check_expr(&f.value, scope, atom_names, warnings);
            }
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                check_expr(&f.value, scope, atom_names, warnings);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            check_expr(base, scope, atom_names, warnings);
        }
        ast::Expr::ListComp(body_expr, qualifiers, _)
        | ast::Expr::BinComp(body_expr, qualifiers, _) => {
            let mut lc_scope = scope.clone();
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(pat, source_expr, _)
                    | ast::LcQualifier::BinGenerator(pat, source_expr, _) => {
                        check_expr(source_expr, &lc_scope, atom_names, warnings);
                        check_pattern_for_underscore_reuse(
                            pat, &lc_scope, atom_names, warnings,
                        );
                        collect_pattern_bindings(pat, &mut lc_scope);
                    }
                    ast::LcQualifier::Filter(filter_expr, _) => {
                        check_expr(filter_expr, &lc_scope, atom_names, warnings);
                    }
                }
            }
            check_expr(body_expr, &lc_scope, atom_names, warnings);
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                check_expr(&seg.value, scope, atom_names, warnings);
                if let Some(ref size) = seg.size {
                    check_expr(size, scope, atom_names, warnings);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                check_expr(&f.key, scope, atom_names, warnings);
                check_expr(&f.value, scope, atom_names, warnings);
            }
        }
        ast::Expr::Maybe {
            body,
            else_clauses,
            ..
        } => {
            let mut maybe_scope = scope.clone();
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => {
                        check_expr(e, &maybe_scope, atom_names, warnings);
                    }
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        check_expr(e, &maybe_scope, atom_names, warnings);
                        check_pattern_for_underscore_reuse(
                            pat, &maybe_scope, atom_names, warnings,
                        );
                        collect_pattern_bindings(pat, &mut maybe_scope);
                    }
                }
            }
            for clause in else_clauses {
                check_clause_patterns_and_body(
                    std::slice::from_ref(&clause.pattern),
                    &clause.body,
                    scope,
                    atom_names,
                    warnings,
                );
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
            check_expr(m, scope, atom_names, warnings);
            check_expr(f, scope, atom_names, warnings);
            check_expr(a, scope, atom_names, warnings);
        }
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    check_expr(arg, scope, atom_names, warnings);
                }
            }
        }
    }
}

/// Check patterns of a clause (case/receive/try arm) against the outer scope,
/// then recurse into the clause body with extended scope.
fn check_clause_patterns_and_body(
    patterns: &[ast::Pattern],
    body: &[ast::Expr],
    outer_scope: &HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    let mut scope = outer_scope.clone();
    for pat in patterns {
        check_pattern_for_underscore_reuse(pat, &scope, atom_names, warnings);
        collect_pattern_bindings(pat, &mut scope);
    }
    check_body(body, &mut scope, atom_names, warnings);
}

fn emit_warning(name: &str, span: Span, warnings: &mut Vec<Diagnostic>) {
    warnings.push(
        Diagnostic::warning_at(
            format!("variable '{}' already bound — did you mean '_'?", name),
            span,
        )
        .with_code("match_underscore_var")
        .with_hint(
            "use '_' to ignore the value, or remove the underscore to match against the bound value"
                .to_string(),
        ),
    );
}
