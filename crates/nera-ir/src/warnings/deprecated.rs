//! Deprecated function call detection.
//!
//! Warns when a function marked with `-deprecated([fun/arity, ...]).`
//! is called or referenced locally within the same module.

use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;

/// Check for calls and references to deprecated functions.
pub(super) fn check_deprecated_calls(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    if module.deprecated.is_empty() {
        return;
    }

    let deprecated: HashSet<(AtomId, u32)> = module
        .deprecated
        .iter()
        .map(|fa| (fa.name, fa.arity))
        .collect();

    for func in &module.functions {
        for clause in &func.clauses {
            if let Some(ref guard) = clause.guard {
                check_guard(guard, &deprecated, atom_names, warnings);
            }
            for expr in &clause.body {
                check_expr(expr, &deprecated, atom_names, warnings);
            }
        }
    }
}

fn warn_deprecated(
    name: AtomId,
    arity: u32,
    span: nera_syntax::span::Span,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    let func_name = atom_names
        .get(&name)
        .map(|s| s.as_str())
        .unwrap_or("?");
    warnings.push(
        Diagnostic::warning_at(
            format!("function '{}/{}' is deprecated", func_name, arity),
            span,
        )
        .with_code("deprecated_function"),
    );
}

fn check_expr(
    expr: &ast::Expr,
    deprecated: &HashSet<(AtomId, u32)>,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    match expr {
        ast::Expr::Call(target, args, span) => {
            if let ast::CallTarget::Local(name, _) = target {
                let arity = args.len() as u32;
                if deprecated.contains(&(*name, arity)) {
                    warn_deprecated(*name, arity, *span, atom_names, warnings);
                }
            }
            // Recurse into arguments and remote call sub-expressions.
            if let ast::CallTarget::Remote(m, f) = target {
                check_expr(m, deprecated, atom_names, warnings);
                check_expr(f, deprecated, atom_names, warnings);
            }
            for arg in args {
                check_expr(arg, deprecated, atom_names, warnings);
            }
        }
        ast::Expr::FunRef(ast::FunRefTarget::Local(name, arity), span) => {
            if deprecated.contains(&(*name, *arity)) {
                warn_deprecated(*name, *arity, *span, atom_names, warnings);
            }
        }
        // Recurse into sub-expressions.
        ast::Expr::Tuple(exprs, _) | ast::Expr::Block(exprs, _) => {
            for e in exprs {
                check_expr(e, deprecated, atom_names, warnings);
            }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) | ast::Expr::BinOp(_, h, t, _) => {
            check_expr(h, deprecated, atom_names, warnings);
            check_expr(t, deprecated, atom_names, warnings);
        }
        ast::Expr::Match(_, e, _)
        | ast::Expr::UnaryOp(_, e, _)
        | ast::Expr::Throw(e, _)
        | ast::Expr::Catch(e, _) => {
            check_expr(e, deprecated, atom_names, warnings);
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            check_expr(scrutinee, deprecated, atom_names, warnings);
            for clause in clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, deprecated, atom_names, warnings);
                }
                for e in &clause.body {
                    check_expr(e, deprecated, atom_names, warnings);
                }
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                check_guard(&clause.guard, deprecated, atom_names, warnings);
                for e in &clause.body {
                    check_expr(e, deprecated, atom_names, warnings);
                }
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, deprecated, atom_names, warnings);
                }
                for e in &clause.body {
                    check_expr(e, deprecated, atom_names, warnings);
                }
            }
            if let Some(after) = after {
                check_expr(&after.timeout, deprecated, atom_names, warnings);
                for e in &after.body {
                    check_expr(e, deprecated, atom_names, warnings);
                }
            }
        }
        ast::Expr::Try {
            body,
            of_clauses,
            catch_clauses,
            after_body,
            ..
        } => {
            for e in body {
                check_expr(e, deprecated, atom_names, warnings);
            }
            for clause in of_clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, deprecated, atom_names, warnings);
                }
                for e in &clause.body {
                    check_expr(e, deprecated, atom_names, warnings);
                }
            }
            for clause in catch_clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, deprecated, atom_names, warnings);
                }
                for e in &clause.body {
                    check_expr(e, deprecated, atom_names, warnings);
                }
            }
            if let Some(after) = after_body {
                for e in after {
                    check_expr(e, deprecated, atom_names, warnings);
                }
            }
        }
        ast::Expr::Fun(_, clauses, _) => {
            for clause in clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, deprecated, atom_names, warnings);
                }
                for e in &clause.body {
                    check_expr(e, deprecated, atom_names, warnings);
                }
            }
        }
        ast::Expr::ListComp(body, qualifiers, _) | ast::Expr::BinComp(body, qualifiers, _) => {
            check_expr(body, deprecated, atom_names, warnings);
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(_, e, _)
                    | ast::LcQualifier::BinGenerator(_, e, _)
                    | ast::LcQualifier::Filter(e, _) => {
                        check_expr(e, deprecated, atom_names, warnings);
                    }
                }
            }
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                check_expr(&seg.value, deprecated, atom_names, warnings);
                if let Some(ref size) = seg.size {
                    check_expr(size, deprecated, atom_names, warnings);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                check_expr(&f.key, deprecated, atom_names, warnings);
                check_expr(&f.value, deprecated, atom_names, warnings);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_expr(base, deprecated, atom_names, warnings);
            for f in fields {
                check_expr(&f.key, deprecated, atom_names, warnings);
                check_expr(&f.value, deprecated, atom_names, warnings);
            }
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                check_expr(&f.value, deprecated, atom_names, warnings);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            check_expr(base, deprecated, atom_names, warnings);
            for f in fields {
                check_expr(&f.value, deprecated, atom_names, warnings);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            check_expr(base, deprecated, atom_names, warnings);
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => check_expr(e, deprecated, atom_names, warnings),
                    ast::MaybeExpr::CondMatch(_, e, _) => {
                        check_expr(e, deprecated, atom_names, warnings)
                    }
                }
            }
            for clause in else_clauses {
                for e in &clause.body {
                    check_expr(e, deprecated, atom_names, warnings);
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
        | ast::Expr::FunRef(ast::FunRefTarget::Remote(..), _)
        | ast::Expr::RecordIndex { .. } => {}
        ast::Expr::FunRef(ast::FunRefTarget::DynamicRemote(m, f, a), _) => {
            check_expr(m, deprecated, atom_names, warnings);
            check_expr(f, deprecated, atom_names, warnings);
            check_expr(a, deprecated, atom_names, warnings);
        }
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    check_expr(arg, deprecated, atom_names, warnings);
                }
            }
        }
    }
}

fn check_guard(
    guard: &ast::Guard,
    deprecated: &HashSet<(AtomId, u32)>,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    match guard {
        ast::Guard::Test(e) => check_expr(e, deprecated, atom_names, warnings),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                check_expr(e, deprecated, atom_names, warnings);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                check_guard(g, deprecated, atom_names, warnings);
            }
        }
    }
}
