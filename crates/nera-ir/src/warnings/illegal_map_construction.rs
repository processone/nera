//! Validates map construction expressions.
//!
//! In Erlang, map construction (`#{k => v}`) must use `=>` (association).
//! Using `:=` (exact match) in map construction is invalid — OTP emits
//! `illegal_map_construction`. Map updates (`M#{k := v}`) may use either
//! operator.

use nera_syntax::ast::{self, MapFieldOp};
use nera_syntax::error::Diagnostic;

/// Check all map constructions in a function for illegal `:=` usage.
pub(super) fn check_illegal_map_construction_fn(
    func: &ast::Function,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for clause in &func.clauses {
        if let Some(guard) = &clause.guard {
            check_guard(guard, diagnostics);
        }
        for expr in &clause.body {
            check_expr(expr, diagnostics);
        }
    }
}

fn check_guard(guard: &ast::Guard, diagnostics: &mut Vec<Diagnostic>) {
    match guard {
        ast::Guard::Test(expr) => check_expr(expr, diagnostics),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                check_expr(e, diagnostics);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                check_guard(g, diagnostics);
            }
        }
    }
}

fn check_expr(expr: &ast::Expr, diagnostics: &mut Vec<Diagnostic>) {
    match expr {
        ast::Expr::Map(fields, _) => {
            for f in fields {
                if f.op == MapFieldOp::Exact {
                    diagnostics.push(
                        Diagnostic::error_at(
                            "illegal use of `:=` in map construction; use `=>` instead",
                            f.op_span,
                        )
                        .with_code("illegal_map_construction"),
                    );
                }
                check_expr(&f.key, diagnostics);
                check_expr(&f.value, diagnostics);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_expr(base, diagnostics);
            for f in fields {
                check_expr(&f.key, diagnostics);
                check_expr(&f.value, diagnostics);
            }
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            check_expr(scrutinee, diagnostics);
            for clause in clauses {
                for e in &clause.body {
                    check_expr(e, diagnostics);
                }
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                for e in &clause.body {
                    check_expr(e, diagnostics);
                }
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                for e in &clause.body {
                    check_expr(e, diagnostics);
                }
            }
            if let Some(after) = after {
                check_expr(&after.timeout, diagnostics);
                for e in &after.body {
                    check_expr(e, diagnostics);
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
                check_expr(e, diagnostics);
            }
            for clause in of_clauses {
                for e in &clause.body {
                    check_expr(e, diagnostics);
                }
            }
            for clause in catch_clauses {
                for e in &clause.body {
                    check_expr(e, diagnostics);
                }
            }
            if let Some(after) = after_body {
                for e in after {
                    check_expr(e, diagnostics);
                }
            }
        }
        ast::Expr::Fun(_, clauses, _) => {
            for clause in clauses {
                for e in &clause.body {
                    check_expr(e, diagnostics);
                }
            }
        }
        ast::Expr::Block(exprs, _) => {
            for e in exprs {
                check_expr(e, diagnostics);
            }
        }
        ast::Expr::Match(_, rhs, _) => {
            check_expr(rhs, diagnostics);
        }
        ast::Expr::Tuple(exprs, _) => {
            for e in exprs {
                check_expr(e, diagnostics);
            }
        }
        ast::Expr::Cons(hd, tl, _) => {
            check_expr(hd, diagnostics);
            check_expr(tl, diagnostics);
        }
        ast::Expr::BinOp(_, lhs, rhs, _) => {
            check_expr(lhs, diagnostics);
            check_expr(rhs, diagnostics);
        }
        ast::Expr::UnaryOp(_, operand, _) => {
            check_expr(operand, diagnostics);
        }
        ast::Expr::Call(_, args, _) => {
            for a in args {
                check_expr(a, diagnostics);
            }
        }
        ast::Expr::Send(lhs, rhs, _) => {
            check_expr(lhs, diagnostics);
            check_expr(rhs, diagnostics);
        }
        ast::Expr::ListComp(expr, qualifiers, _) | ast::Expr::BinComp(expr, qualifiers, _) => {
            check_expr(expr, diagnostics);
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(_, e, _)
                    | ast::LcQualifier::BinGenerator(_, e, _) => {
                        check_expr(e, diagnostics);
                    }
                    ast::LcQualifier::Filter(e, _) => {
                        check_expr(e, diagnostics);
                    }
                }
            }
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                check_expr(&seg.value, diagnostics);
                if let Some(size) = &seg.size {
                    check_expr(size, diagnostics);
                }
            }
        }
        ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            check_expr(e, diagnostics);
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                check_expr(&f.value, diagnostics);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            check_expr(base, diagnostics);
            for f in fields {
                check_expr(&f.value, diagnostics);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            check_expr(base, diagnostics);
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => check_expr(e, diagnostics),
                    ast::MaybeExpr::CondMatch(_, e, _) => {
                        check_expr(e, diagnostics);
                    }
                }
            }
            for clause in else_clauses {
                for e in &clause.body {
                    check_expr(e, diagnostics);
                }
            }
        }
        _ => {}
    }
}
