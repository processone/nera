//! Unsafe variable detection.
//!
//! In Erlang, a variable is "unsafe" when it is bound inside some but not all
//! branches of a branching expression (`case`, `if`, `receive`, `try`) and is
//! then referenced after the expression.  OTP's `erl_lint` emits:
//!
//! ```text
//! variable 'Y' unsafe in 'case' (line N)
//! ```
//!
//! This pass walks each function body and detects this pattern.  It collects
//! bindings introduced by each branch, computes which variables are *not*
//! bound in every branch (the "unsafe" set), and then checks whether any of
//! those variables are referenced in expressions that follow the branching
//! construct. As in `erl_lint`, a fun head, a named fun's name or a
//! comprehension generator pattern binds a fresh name: reads of it inside that
//! fun clause or comprehension are not references to the unsafe variable.

use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;
use nera_syntax::span::Span;

use super::ast_walkers::{collect_enclosing_refs, collect_pattern_bindings};

/// Check a function for unsafe variable references.
pub(super) fn check_unsafe_variables_fn(
    func: &ast::Function,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for clause in &func.clauses {
        let mut scope = HashSet::new();
        for pat in &clause.patterns {
            collect_pattern_bindings(pat, &mut scope);
        }
        check_unsafe_in_body(&clause.body, &scope, atom_names, diagnostics);
    }
}

/// Walk a body (sequence of expressions) looking for branching constructs
/// that introduce unsafe variables used in subsequent expressions.
fn check_unsafe_in_body(
    body: &[ast::Expr],
    outer_scope: &HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut scope = outer_scope.clone();

    for (i, expr) in body.iter().enumerate() {
        // For match expressions, add the bound variables to the scope.
        if let ast::Expr::Match(pat, _rhs, _) = expr {
            collect_pattern_bindings(pat, &mut scope);
        }

        // Collect unsafe vars introduced by this expression.
        let unsafe_vars = collect_unsafe_from_expr(expr, &scope);

        if !unsafe_vars.is_empty() {
            // Collect reads of these names in the remaining body expressions.
            // A fun head or comprehension generator that binds the same name
            // shadows it, so reads inside that scope do not count.
            let mut later_refs = HashSet::new();
            for later_expr in &body[i + 1..] {
                collect_enclosing_refs(later_expr, &mut later_refs);
            }

            for (var_id, construct, construct_span) in &unsafe_vars {
                if later_refs.contains(var_id) {
                    let name = match atom_names.get(var_id) {
                        Some(s) => s.as_str(),
                        None => continue,
                    };
                    // Skip underscore-prefixed variables.
                    if name == "_" || name.starts_with('_') {
                        continue;
                    }
                    diagnostics.push(
                        Diagnostic::error_at(
                            format!("variable '{}' unsafe in '{}'", name, construct),
                            *construct_span,
                        )
                        .with_code("unsafe_variable".to_string())
                        .with_label(format!("'{}' is bound in some branches but not all", name))
                        .with_hint(format!(
                            "variable '{}' is bound inside a '{}' expression but not in every branch, \
                             so it may not have a value when used after the '{}'",
                            name, construct, construct
                        )),
                    );
                }
            }
        }

        // Recurse into sub-expressions to find nested unsafe patterns.
        check_unsafe_in_subexprs(expr, &scope, atom_names, diagnostics);
    }
}

/// Collect variables that are unsafe coming out of a branching expression.
///
/// Returns (AtomId, construct_name, construct_span) for each variable that
/// is bound in some but not all branches.
fn collect_unsafe_from_expr(
    expr: &ast::Expr,
    scope: &HashSet<AtomId>,
) -> Vec<(AtomId, &'static str, Span)> {
    match expr {
        ast::Expr::Case(_scrutinee, clauses, span) => {
            let branch_bindings: Vec<HashSet<AtomId>> = clauses
                .iter()
                .map(|clause| {
                    let mut binds = HashSet::new();
                    collect_pattern_bindings(&clause.pattern, &mut binds);
                    collect_body_bindings(&clause.body, &mut binds);
                    // Only include variables not already in scope.
                    binds.difference(scope).copied().collect()
                })
                .collect();
            find_unsafe_vars(&branch_bindings, "case", *span)
        }

        ast::Expr::If(clauses, span) => {
            let branch_bindings: Vec<HashSet<AtomId>> = clauses
                .iter()
                .map(|clause| {
                    let mut binds = HashSet::new();
                    collect_body_bindings(&clause.body, &mut binds);
                    binds.difference(scope).copied().collect()
                })
                .collect();
            find_unsafe_vars(&branch_bindings, "if", *span)
        }

        ast::Expr::Receive(clauses, after, span) => {
            let mut branch_bindings: Vec<HashSet<AtomId>> = clauses
                .iter()
                .map(|clause| {
                    let mut binds = HashSet::new();
                    collect_pattern_bindings(&clause.pattern, &mut binds);
                    collect_body_bindings(&clause.body, &mut binds);
                    binds.difference(scope).copied().collect()
                })
                .collect();

            if let Some(after_clause) = after {
                let mut after_binds = HashSet::new();
                collect_body_bindings(&after_clause.body, &mut after_binds);
                branch_bindings.push(after_binds.difference(scope).copied().collect());
            }

            find_unsafe_vars(&branch_bindings, "receive", *span)
        }

        ast::Expr::Try {
            body: _,
            of_clauses,
            catch_clauses,
            after_body: _,
            span,
        } => {
            // In try, variables bound in the try body are unsafe because the
            // catch path may execute instead.  We treat of_clauses and
            // catch_clauses as separate branch sets.
            //
            // Variables are only safe if bound in ALL of_clauses AND ALL
            // catch_clauses (since either path may execute).
            let mut all_branches: Vec<HashSet<AtomId>> = Vec::new();

            for clause in of_clauses {
                let mut binds = HashSet::new();
                collect_pattern_bindings(&clause.pattern, &mut binds);
                collect_body_bindings(&clause.body, &mut binds);
                all_branches.push(binds.difference(scope).copied().collect());
            }

            for clause in catch_clauses {
                let mut binds = HashSet::new();
                collect_pattern_bindings(&clause.pattern, &mut binds);
                if let Some(ref stk) = clause.stacktrace {
                    collect_pattern_bindings(stk, &mut binds);
                }
                collect_body_bindings(&clause.body, &mut binds);
                all_branches.push(binds.difference(scope).copied().collect());
            }

            // If there are no of_clauses and no catch_clauses, we add the
            // try body itself as a branch (the body runs to completion).
            if all_branches.is_empty() {
                return Vec::new();
            }

            find_unsafe_vars(&all_branches, "try", *span)
        }

        _ => Vec::new(),
    }
}

/// Given the set of new bindings from each branch, find variables that appear
/// in some branches but not all (the "unsafe" variables).
fn find_unsafe_vars(
    branch_bindings: &[HashSet<AtomId>],
    construct: &'static str,
    span: Span,
) -> Vec<(AtomId, &'static str, Span)> {
    if branch_bindings.is_empty() {
        return Vec::new();
    }

    // Union of all variables bound across any branch.
    let all_vars: HashSet<AtomId> = branch_bindings
        .iter()
        .flat_map(|s| s.iter().copied())
        .collect();

    // Intersection: variables bound in every branch.
    let safe_vars: HashSet<AtomId> = all_vars
        .iter()
        .copied()
        .filter(|v| branch_bindings.iter().all(|b| b.contains(v)))
        .collect();

    // Unsafe = in some branch but not all.
    all_vars
        .difference(&safe_vars)
        .map(|v| (*v, construct, span))
        .collect()
}

/// Collect variables bound by match expressions in a body (top-level only).
fn collect_body_bindings(body: &[ast::Expr], binds: &mut HashSet<AtomId>) {
    for expr in body {
        if let ast::Expr::Match(pat, _, _) = expr {
            collect_pattern_bindings(pat, binds);
        }
    }
}

/// Recurse into sub-expressions of an expression to check for unsafe vars
/// in nested bodies (fun bodies, case bodies, etc.).
fn check_unsafe_in_subexprs(
    expr: &ast::Expr,
    scope: &HashSet<AtomId>,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match expr {
        ast::Expr::Fun(_name, clauses, _) => {
            for clause in clauses {
                let mut fun_scope = scope.clone();
                for pat in &clause.patterns {
                    collect_pattern_bindings(pat, &mut fun_scope);
                }
                check_unsafe_in_body(&clause.body, &fun_scope, atom_names, diagnostics);
            }
        }

        ast::Expr::Case(scrutinee, clauses, _) => {
            check_unsafe_in_subexprs(scrutinee, scope, atom_names, diagnostics);
            for clause in clauses {
                let mut clause_scope = scope.clone();
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                check_unsafe_in_body(&clause.body, &clause_scope, atom_names, diagnostics);
            }
        }

        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                check_unsafe_in_body(&clause.body, scope, atom_names, diagnostics);
            }
        }

        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                let mut clause_scope = scope.clone();
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                check_unsafe_in_body(&clause.body, &clause_scope, atom_names, diagnostics);
            }
            if let Some(after) = after {
                check_unsafe_in_body(&after.body, scope, atom_names, diagnostics);
            }
        }

        ast::Expr::Try {
            body,
            of_clauses,
            catch_clauses,
            after_body,
            ..
        } => {
            check_unsafe_in_body(body, scope, atom_names, diagnostics);
            for clause in of_clauses {
                let mut clause_scope = scope.clone();
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                check_unsafe_in_body(&clause.body, &clause_scope, atom_names, diagnostics);
            }
            for clause in catch_clauses {
                let mut clause_scope = scope.clone();
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                if let Some(ref stk) = clause.stacktrace {
                    collect_pattern_bindings(stk, &mut clause_scope);
                }
                check_unsafe_in_body(&clause.body, &clause_scope, atom_names, diagnostics);
            }
            if let Some(after) = after_body {
                check_unsafe_in_body(after, scope, atom_names, diagnostics);
            }
        }

        ast::Expr::ListComp(body_expr, qualifiers, _)
        | ast::Expr::BinComp(body_expr, qualifiers, _) => {
            let mut lc_scope = scope.clone();
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(pat, source_expr, _)
                    | ast::LcQualifier::BinGenerator(pat, source_expr, _) => {
                        check_unsafe_in_subexprs(source_expr, &lc_scope, atom_names, diagnostics);
                        collect_pattern_bindings(pat, &mut lc_scope);
                    }
                    ast::LcQualifier::Filter(filter_expr, _) => {
                        check_unsafe_in_subexprs(filter_expr, &lc_scope, atom_names, diagnostics);
                    }
                }
            }
            check_unsafe_in_subexprs(body_expr, &lc_scope, atom_names, diagnostics);
        }

        ast::Expr::Maybe { body, else_clauses, .. } => {
            let mut maybe_scope = scope.clone();
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => {
                        if let ast::Expr::Match(pat, _, _) = e {
                            collect_pattern_bindings(pat, &mut maybe_scope);
                        }
                        check_unsafe_in_subexprs(e, &maybe_scope, atom_names, diagnostics);
                    }
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        check_unsafe_in_subexprs(e, &maybe_scope, atom_names, diagnostics);
                        collect_pattern_bindings(pat, &mut maybe_scope);
                    }
                }
            }
            for clause in else_clauses {
                let mut clause_scope = scope.clone();
                collect_pattern_bindings(&clause.pattern, &mut clause_scope);
                check_unsafe_in_body(&clause.body, &clause_scope, atom_names, diagnostics);
            }
        }

        ast::Expr::Block(exprs, _) => {
            check_unsafe_in_body(exprs, scope, atom_names, diagnostics);
        }

        // Binary sub-expressions.
        ast::Expr::Match(_, rhs, _) => {
            check_unsafe_in_subexprs(rhs, scope, atom_names, diagnostics);
        }
        ast::Expr::Tuple(exprs, _) => {
            for e in exprs {
                check_unsafe_in_subexprs(e, scope, atom_names, diagnostics);
            }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) | ast::Expr::BinOp(_, h, t, _) => {
            check_unsafe_in_subexprs(h, scope, atom_names, diagnostics);
            check_unsafe_in_subexprs(t, scope, atom_names, diagnostics);
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            check_unsafe_in_subexprs(e, scope, atom_names, diagnostics);
        }
        ast::Expr::Call(target, args, _) => {
            if let ast::CallTarget::Remote(module_expr, fun_expr) = target {
                check_unsafe_in_subexprs(module_expr, scope, atom_names, diagnostics);
                check_unsafe_in_subexprs(fun_expr, scope, atom_names, diagnostics);
            }
            for arg in args {
                check_unsafe_in_subexprs(arg, scope, atom_names, diagnostics);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_unsafe_in_subexprs(base, scope, atom_names, diagnostics);
            for f in fields {
                check_unsafe_in_subexprs(&f.key, scope, atom_names, diagnostics);
                check_unsafe_in_subexprs(&f.value, scope, atom_names, diagnostics);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            check_unsafe_in_subexprs(base, scope, atom_names, diagnostics);
            for f in fields {
                check_unsafe_in_subexprs(&f.value, scope, atom_names, diagnostics);
            }
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                check_unsafe_in_subexprs(&f.value, scope, atom_names, diagnostics);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            check_unsafe_in_subexprs(base, scope, atom_names, diagnostics);
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                check_unsafe_in_subexprs(&f.key, scope, atom_names, diagnostics);
                check_unsafe_in_subexprs(&f.value, scope, atom_names, diagnostics);
            }
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                check_unsafe_in_subexprs(&seg.value, scope, atom_names, diagnostics);
                if let Some(ref size) = seg.size {
                    check_unsafe_in_subexprs(size, scope, atom_names, diagnostics);
                }
            }
        }

        // Leaf nodes.
        ast::Expr::Integer(_, _)
        | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(_, _)
        | ast::Expr::Atom(_, _)
        | ast::Expr::String(_, _)
        | ast::Expr::Nil(_)
        | ast::Expr::Variable(_, _)
        | ast::Expr::FunRef(ast::FunRefTarget::Local(..) | ast::FunRefTarget::Remote(..), _)
        | ast::Expr::RecordIndex { .. } => {}
        ast::Expr::FunRef(ast::FunRefTarget::DynamicRemote(m, f, a), _) => {
            check_unsafe_in_subexprs(m, scope, atom_names, diagnostics);
            check_unsafe_in_subexprs(f, scope, atom_names, diagnostics);
            check_unsafe_in_subexprs(a, scope, atom_names, diagnostics);
        }
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    check_unsafe_in_subexprs(arg, scope, atom_names, diagnostics);
                }
            }
        }
    }
}
