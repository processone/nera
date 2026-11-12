//! Non-tail-recursion detection.
//!
//! Warns when a function calls itself recursively but the call is not in
//! tail position.  Such calls grow the stack on every iteration and are
//! often unintentional – the developer meant to use an accumulator pattern.
//!
//! Also provides [`is_recursive`] which simply tests whether a function
//! contains *any* direct self-call (regardless of tail position), used by
//! the LSP to annotate document symbols.

use std::collections::HashMap;

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;

/// Emit warnings for non-tail-recursive calls inside `func`.
pub(super) fn check_non_tail_recursion_fn(
    func: &ast::Function,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    let display = atom_names
        .get(&func.name)
        .map(|s| s.as_str())
        .unwrap_or("?");

    for clause in &func.clauses {
        check_body(&clause.body, true, func.name, func.arity, display, atom_names, warnings);
    }
}

/// Returns `true` if the function contains any direct self-recursive call.
pub fn is_recursive(func: &ast::Function) -> bool {
    func.clauses.iter().any(|c| body_has_self_call(&c.body, func.name, func.arity))
}

// ---------------------------------------------------------------------------
// Tail-position-aware walk
// ---------------------------------------------------------------------------

/// Context threaded through the walk to avoid long parameter lists.
struct Ctx<'a> {
    self_name: AtomId,
    self_arity: u32,
    display: &'a str,
    atom_names: &'a HashMap<AtomId, String>,
}

fn check_body(
    body: &[ast::Expr],
    is_tail: bool,
    self_name: AtomId,
    self_arity: u32,
    display: &str,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    let ctx = Ctx { self_name, self_arity, display, atom_names };
    check_body_ctx(body, is_tail, &ctx, warnings);
}

fn check_body_ctx(
    body: &[ast::Expr],
    is_tail: bool,
    ctx: &Ctx,
    warnings: &mut Vec<Diagnostic>,
) {
    if let Some((last, rest)) = body.split_last() {
        for expr in rest {
            walk(expr, false, ctx, warnings);
        }
        walk(last, is_tail, ctx, warnings);
    }
}

fn walk(
    expr: &ast::Expr,
    is_tail: bool,
    ctx: &Ctx,
    w: &mut Vec<Diagnostic>,
) {
    match expr {
        // -- The crucial check: a local call matching self name/arity --
        ast::Expr::Call(ast::CallTarget::Local(name, _), args, span) => {
            if *name == ctx.self_name && args.len() as u32 == ctx.self_arity && !is_tail {
                w.push(
                    Diagnostic::warning_at(
                        format!(
                            "non-tail-recursive call to '{}/{}'",
                            ctx.display, ctx.self_arity,
                        ),
                        *span,
                    )
                    .with_code("non_tail_recursion")
                    .with_hint(
                        "this call is not in tail position and will grow the stack; \
                         consider using an accumulator",
                    ),
                );
            }
            for arg in args {
                walk(arg, false, ctx, w);
            }
        }

        // Other calls – walk sub-expressions, none in tail position
        ast::Expr::Call(ast::CallTarget::Remote(m, f), args, _) => {
            walk(m, false, ctx, w);
            walk(f, false, ctx, w);
            for arg in args {
                walk(arg, false, ctx, w);
            }
        }
        ast::Expr::Call(ast::CallTarget::Variable(_, _), args, _) => {
            for arg in args {
                walk(arg, false, ctx, w);
            }
        }
        ast::Expr::Call(ast::CallTarget::Expr(callee), args, _) => {
            walk(callee, false, ctx, w);
            for arg in args {
                walk(arg, false, ctx, w);
            }
        }

        // -- Tail-propagating forms --

        ast::Expr::Match(_, rhs, _) => {
            walk(rhs, is_tail, ctx, w);
        }

        ast::Expr::Block(exprs, _) => {
            check_body_ctx(exprs, is_tail, ctx, w);
        }

        ast::Expr::Case(scrutinee, clauses, _) => {
            walk(scrutinee, false, ctx, w);
            for clause in clauses {
                check_body_ctx(&clause.body, is_tail, ctx, w);
            }
        }

        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                check_body_ctx(&clause.body, is_tail, ctx, w);
            }
        }

        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                check_body_ctx(&clause.body, is_tail, ctx, w);
            }
            if let Some(after) = after {
                walk(&after.timeout, false, ctx, w);
                check_body_ctx(&after.body, is_tail, ctx, w);
            }
        }

        ast::Expr::Try {
            body,
            of_clauses,
            catch_clauses,
            after_body,
            ..
        } => {
            // try body is NOT in tail position
            check_body_ctx(body, false, ctx, w);
            for clause in of_clauses {
                check_body_ctx(&clause.body, is_tail, ctx, w);
            }
            for clause in catch_clauses {
                check_body_ctx(&clause.body, is_tail, ctx, w);
            }
            // after body is NEVER in tail position
            if let Some(after) = after_body {
                check_body_ctx(after, false, ctx, w);
            }
        }

        ast::Expr::Maybe { body, else_clauses, .. } => {
            if let Some((last, rest)) = body.split_last() {
                for item in rest {
                    walk_maybe_expr(item, false, ctx, w);
                }
                walk_maybe_expr(last, is_tail, ctx, w);
            }
            for clause in else_clauses {
                check_body_ctx(&clause.body, is_tail, ctx, w);
            }
        }

        // -- Named funs: nested check with the fun's own identity --
        ast::Expr::Fun(Some(fun_name), clauses, _) => {
            let fun_arity = clauses.first().map_or(0, |c| c.patterns.len() as u32);
            let fun_display = ctx.atom_names
                .get(fun_name)
                .map(|s| s.as_str())
                .unwrap_or("?");
            let fun_ctx = Ctx {
                self_name: *fun_name,
                self_arity: fun_arity,
                display: fun_display,
                atom_names: ctx.atom_names,
            };
            for clause in clauses {
                check_body_ctx(&clause.body, true, &fun_ctx, w);
            }
        }

        // Anonymous funs – walk body for calls to the *enclosing* function
        ast::Expr::Fun(None, clauses, _) => {
            for clause in clauses {
                for e in &clause.body {
                    walk(e, false, ctx, w);
                }
            }
        }

        // -- Non-tail forms: all children are NOT in tail position --

        ast::Expr::Cons(h, t, _)
        | ast::Expr::Send(h, t, _)
        | ast::Expr::BinOp(_, h, t, _) => {
            walk(h, false, ctx, w);
            walk(t, false, ctx, w);
        }

        ast::Expr::UnaryOp(_, e, _)
        | ast::Expr::Throw(e, _)
        | ast::Expr::Catch(e, _) => {
            walk(e, false, ctx, w);
        }

        ast::Expr::Tuple(exprs, _) => {
            for e in exprs {
                walk(e, false, ctx, w);
            }
        }

        ast::Expr::ListComp(body, qualifiers, _)
        | ast::Expr::BinComp(body, qualifiers, _) => {
            walk(body, false, ctx, w);
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(_, e, _)
                    | ast::LcQualifier::BinGenerator(_, e, _)
                    | ast::LcQualifier::Filter(e, _) => {
                        walk(e, false, ctx, w);
                    }
                }
            }
        }

        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                walk(&seg.value, false, ctx, w);
                if let Some(ref size) = seg.size {
                    walk(size, false, ctx, w);
                }
            }
        }

        ast::Expr::Map(fields, _) => {
            for f in fields {
                walk(&f.key, false, ctx, w);
                walk(&f.value, false, ctx, w);
            }
        }

        ast::Expr::MapUpdate(base, fields, _) => {
            walk(base, false, ctx, w);
            for f in fields {
                walk(&f.key, false, ctx, w);
                walk(&f.value, false, ctx, w);
            }
        }

        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                walk(&f.value, false, ctx, w);
            }
        }

        ast::Expr::RecordUpdate { base, fields, .. } => {
            walk(base, false, ctx, w);
            for f in fields {
                walk(&f.value, false, ctx, w);
            }
        }

        ast::Expr::RecordAccess { base, .. } => {
            walk(base, false, ctx, w);
        }

        // Leaf nodes – nothing to check
        ast::Expr::Variable(_, _)
        | ast::Expr::Integer(_, _)
        | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(_, _)
        | ast::Expr::Atom(_, _)
        | ast::Expr::String(_, _)
        | ast::Expr::Nil(_)
        | ast::Expr::FunRef(_, _)  // sub-exprs in DynamicRemote are atoms/vars/ints — no self-calls
        | ast::Expr::RecordIndex { .. } => {}
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    walk(arg, false, ctx, w);
                }
            }
        }
    }
}

fn walk_maybe_expr(
    item: &ast::MaybeExpr,
    is_tail: bool,
    ctx: &Ctx,
    w: &mut Vec<Diagnostic>,
) {
    match item {
        ast::MaybeExpr::Expr(e) => walk(e, is_tail, ctx, w),
        // The rhs of `?=` is never in tail position (it feeds into the match)
        ast::MaybeExpr::CondMatch(_, e, _) => walk(e, false, ctx, w),
    }
}

// ---------------------------------------------------------------------------
// Simple recursion predicate (ignores tail position)
// ---------------------------------------------------------------------------

fn body_has_self_call(body: &[ast::Expr], self_name: AtomId, self_arity: u32) -> bool {
    body.iter().any(|e| expr_has_self_call(e, self_name, self_arity))
}

fn expr_has_self_call(expr: &ast::Expr, self_name: AtomId, self_arity: u32) -> bool {
    match expr {
        ast::Expr::Call(ast::CallTarget::Local(name, _), args, _) => {
            if *name == self_name && args.len() as u32 == self_arity {
                return true;
            }
            args.iter().any(|a| expr_has_self_call(a, self_name, self_arity))
        }
        ast::Expr::Call(ast::CallTarget::Remote(m, f), args, _) => {
            expr_has_self_call(m, self_name, self_arity)
                || expr_has_self_call(f, self_name, self_arity)
                || args.iter().any(|a| expr_has_self_call(a, self_name, self_arity))
        }
        ast::Expr::Call(ast::CallTarget::Variable(_, _), args, _) => {
            args.iter().any(|a| expr_has_self_call(a, self_name, self_arity))
        }
        ast::Expr::Call(ast::CallTarget::Expr(callee), args, _) => {
            expr_has_self_call(callee, self_name, self_arity)
                || args.iter().any(|a| expr_has_self_call(a, self_name, self_arity))
        }

        ast::Expr::Cons(h, t, _)
        | ast::Expr::Send(h, t, _)
        | ast::Expr::BinOp(_, h, t, _) => {
            expr_has_self_call(h, self_name, self_arity)
                || expr_has_self_call(t, self_name, self_arity)
        }

        ast::Expr::Match(_, e, _)
        | ast::Expr::UnaryOp(_, e, _)
        | ast::Expr::Throw(e, _)
        | ast::Expr::Catch(e, _) => {
            expr_has_self_call(e, self_name, self_arity)
        }

        ast::Expr::Tuple(exprs, _) | ast::Expr::Block(exprs, _) => {
            exprs.iter().any(|e| expr_has_self_call(e, self_name, self_arity))
        }

        ast::Expr::Case(scrutinee, clauses, _) => {
            expr_has_self_call(scrutinee, self_name, self_arity)
                || clauses.iter().any(|c| body_has_self_call(&c.body, self_name, self_arity))
        }

        ast::Expr::If(clauses, _) => {
            clauses.iter().any(|c| body_has_self_call(&c.body, self_name, self_arity))
        }

        ast::Expr::Receive(clauses, after, _) => {
            clauses.iter().any(|c| body_has_self_call(&c.body, self_name, self_arity))
                || after.as_ref().is_some_and(|a| {
                    expr_has_self_call(&a.timeout, self_name, self_arity)
                        || body_has_self_call(&a.body, self_name, self_arity)
                })
        }

        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            body_has_self_call(body, self_name, self_arity)
                || of_clauses.iter().any(|c| body_has_self_call(&c.body, self_name, self_arity))
                || catch_clauses.iter().any(|c| body_has_self_call(&c.body, self_name, self_arity))
                || after_body.as_ref().is_some_and(|b| body_has_self_call(b, self_name, self_arity))
        }

        ast::Expr::Fun(_, clauses, _) => {
            clauses.iter().any(|c| body_has_self_call(&c.body, self_name, self_arity))
        }

        ast::Expr::ListComp(body, qualifiers, _)
        | ast::Expr::BinComp(body, qualifiers, _) => {
            expr_has_self_call(body, self_name, self_arity)
                || qualifiers.iter().any(|q| match q {
                    ast::LcQualifier::Generator(_, e, _)
                    | ast::LcQualifier::BinGenerator(_, e, _)
                    | ast::LcQualifier::Filter(e, _) => {
                        expr_has_self_call(e, self_name, self_arity)
                    }
                })
        }

        ast::Expr::Binary(segments, _) => {
            segments.iter().any(|seg| {
                expr_has_self_call(&seg.value, self_name, self_arity)
                    || seg.size.as_ref().is_some_and(|s| expr_has_self_call(s, self_name, self_arity))
            })
        }

        ast::Expr::Map(fields, _) => {
            fields.iter().any(|f| {
                expr_has_self_call(&f.key, self_name, self_arity)
                    || expr_has_self_call(&f.value, self_name, self_arity)
            })
        }

        ast::Expr::MapUpdate(base, fields, _) => {
            expr_has_self_call(base, self_name, self_arity)
                || fields.iter().any(|f| {
                    expr_has_self_call(&f.key, self_name, self_arity)
                        || expr_has_self_call(&f.value, self_name, self_arity)
                })
        }

        ast::Expr::RecordCreate { fields, .. } => {
            fields.iter().any(|f| expr_has_self_call(&f.value, self_name, self_arity))
        }

        ast::Expr::RecordUpdate { base, fields, .. } => {
            expr_has_self_call(base, self_name, self_arity)
                || fields.iter().any(|f| expr_has_self_call(&f.value, self_name, self_arity))
        }

        ast::Expr::RecordAccess { base, .. } => {
            expr_has_self_call(base, self_name, self_arity)
        }

        ast::Expr::Maybe { body, else_clauses, .. } => {
            body.iter().any(|item| match item {
                ast::MaybeExpr::Expr(e) => expr_has_self_call(e, self_name, self_arity),
                ast::MaybeExpr::CondMatch(_, e, _) => expr_has_self_call(e, self_name, self_arity),
            }) || else_clauses.iter().any(|c| body_has_self_call(&c.body, self_name, self_arity))
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
        | ast::Expr::RecordIndex { .. } => false,
        ast::Expr::MacroCall { args, .. } => {
            args.as_ref().is_some_and(|args| {
                args.iter().any(|a| expr_has_self_call(a, self_name, self_arity))
            })
        }
    }
}
