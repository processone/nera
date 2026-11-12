//! Detect illegal expressions in guard contexts.
//!
//! Erlang restricts guard expressions to side-effect-free operations: comparisons,
//! type tests, arithmetic, boolean operators, and a specific set of guard BIFs.
//! This pass walks all guards in a module and rejects anything outside that whitelist.

use std::collections::HashMap;

use nera_syntax::ast::{self, AtomId, CallTarget, Guard};
use nera_syntax::error::Diagnostic;
use nera_syntax::span::Span;

/// Guard-safe BIFs: `(module, function, arity)`.
///
/// All of these are auto-imported from `erlang` and legal in guard position.
const GUARD_BIFS: &[(&str, &str, u32)] = &[
    // Type tests
    ("erlang", "is_atom", 1),
    ("erlang", "is_binary", 1),
    ("erlang", "is_bitstring", 1),
    ("erlang", "is_boolean", 1),
    ("erlang", "is_float", 1),
    ("erlang", "is_function", 1),
    ("erlang", "is_function", 2),
    ("erlang", "is_integer", 1),
    ("erlang", "is_list", 1),
    ("erlang", "is_map", 1),
    ("erlang", "is_map_key", 2),
    ("erlang", "is_number", 1),
    ("erlang", "is_pid", 1),
    ("erlang", "is_port", 1),
    ("erlang", "is_record", 2),
    ("erlang", "is_record", 3),
    ("erlang", "is_reference", 1),
    ("erlang", "is_tuple", 1),
    // Other guard BIFs
    ("erlang", "abs", 1),
    ("erlang", "binary_part", 2),
    ("erlang", "binary_part", 3),
    ("erlang", "ceil", 1),
    ("erlang", "floor", 1),
    ("erlang", "is_integer", 3),
    ("erlang", "is_record", 1),
    ("erlang", "bit_size", 1),
    ("erlang", "byte_size", 1),
    ("erlang", "element", 2),
    ("erlang", "float", 1),
    ("erlang", "hd", 1),
    ("erlang", "length", 1),
    ("erlang", "map_get", 2),
    ("erlang", "map_size", 1),
    ("erlang", "max", 2),
    ("erlang", "min", 2),
    ("erlang", "node", 0),
    ("erlang", "node", 1),
    ("erlang", "round", 1),
    ("erlang", "self", 0),
    ("erlang", "size", 1),
    ("erlang", "tl", 1),
    ("erlang", "trunc", 1),
    ("erlang", "tuple_size", 1),
];

/// Check all guards in a function for illegal expressions.
pub(super) fn check_illegal_guards_fn(
    func: &ast::Function,
    atom_names: &HashMap<AtomId, String>,
    policy: &crate::resolution::ResolutionPolicy,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for clause in &func.clauses {
        if let Some(guard) = &clause.guard {
            validate_guard(guard, atom_names, policy, diagnostics);
        }
    }
    // Also check guards in nested expressions (case, if, receive, try, fun).
    for clause in &func.clauses {
        for expr in &clause.body {
            check_nested_guards(expr, atom_names, policy, diagnostics);
        }
    }
}

/// Recurse through any expression to find nested guards (case/if/receive/try/fun clauses).
fn check_nested_guards(
    expr: &ast::Expr,
    atom_names: &HashMap<AtomId, String>,
    policy: &crate::resolution::ResolutionPolicy,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match expr {
        ast::Expr::Case(scrutinee, clauses, _) => {
            check_nested_guards(scrutinee, atom_names, policy, diagnostics);
            for clause in clauses {
                if let Some(guard) = &clause.guard {
                    validate_guard(guard, atom_names, policy, diagnostics);
                }
                for e in &clause.body {
                    check_nested_guards(e, atom_names, policy, diagnostics);
                }
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                validate_guard(&clause.guard, atom_names, policy, diagnostics);
                for e in &clause.body {
                    check_nested_guards(e, atom_names, policy, diagnostics);
                }
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                if let Some(guard) = &clause.guard {
                    validate_guard(guard, atom_names, policy, diagnostics);
                }
                for e in &clause.body {
                    check_nested_guards(e, atom_names, policy, diagnostics);
                }
            }
            if let Some(after) = after {
                check_nested_guards(&after.timeout, atom_names, policy, diagnostics);
                for e in &after.body {
                    check_nested_guards(e, atom_names, policy, diagnostics);
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
                check_nested_guards(e, atom_names, policy, diagnostics);
            }
            for clause in of_clauses {
                if let Some(guard) = &clause.guard {
                    validate_guard(guard, atom_names, policy, diagnostics);
                }
                for e in &clause.body {
                    check_nested_guards(e, atom_names, policy, diagnostics);
                }
            }
            for clause in catch_clauses {
                if let Some(guard) = &clause.guard {
                    validate_guard(guard, atom_names, policy, diagnostics);
                }
                for e in &clause.body {
                    check_nested_guards(e, atom_names, policy, diagnostics);
                }
            }
            if let Some(after) = after_body {
                for e in after {
                    check_nested_guards(e, atom_names, policy, diagnostics);
                }
            }
        }
        ast::Expr::Fun(_, clauses, _) => {
            for clause in clauses {
                if let Some(guard) = &clause.guard {
                    validate_guard(guard, atom_names, policy, diagnostics);
                }
                for e in &clause.body {
                    check_nested_guards(e, atom_names, policy, diagnostics);
                }
            }
        }
        ast::Expr::Block(exprs, _) => {
            for e in exprs {
                check_nested_guards(e, atom_names, policy, diagnostics);
            }
        }
        ast::Expr::Match(_, rhs, _) => {
            check_nested_guards(rhs, atom_names, policy, diagnostics);
        }
        ast::Expr::Tuple(exprs, _) => {
            for e in exprs {
                check_nested_guards(e, atom_names, policy, diagnostics);
            }
        }
        ast::Expr::Cons(hd, tl, _) => {
            check_nested_guards(hd, atom_names, policy, diagnostics);
            check_nested_guards(tl, atom_names, policy, diagnostics);
        }
        ast::Expr::BinOp(_, lhs, rhs, _) => {
            check_nested_guards(lhs, atom_names, policy, diagnostics);
            check_nested_guards(rhs, atom_names, policy, diagnostics);
        }
        ast::Expr::UnaryOp(_, operand, _) => {
            check_nested_guards(operand, atom_names, policy, diagnostics);
        }
        ast::Expr::Call(_, args, _) => {
            for a in args {
                check_nested_guards(a, atom_names, policy, diagnostics);
            }
        }
        ast::Expr::Send(lhs, rhs, _) => {
            check_nested_guards(lhs, atom_names, policy, diagnostics);
            check_nested_guards(rhs, atom_names, policy, diagnostics);
        }
        ast::Expr::ListComp(expr, qualifiers, _) | ast::Expr::BinComp(expr, qualifiers, _) => {
            check_nested_guards(expr, atom_names, policy, diagnostics);
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(_, e, _)
                    | ast::LcQualifier::BinGenerator(_, e, _) => {
                        check_nested_guards(e, atom_names, policy, diagnostics);
                    }
                    ast::LcQualifier::Filter(e, _) => {
                        check_nested_guards(e, atom_names, policy, diagnostics);
                    }
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                check_nested_guards(&f.key, atom_names, policy, diagnostics);
                check_nested_guards(&f.value, atom_names, policy, diagnostics);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_nested_guards(base, atom_names, policy, diagnostics);
            for f in fields {
                check_nested_guards(&f.key, atom_names, policy, diagnostics);
                check_nested_guards(&f.value, atom_names, policy, diagnostics);
            }
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                check_nested_guards(&seg.value, atom_names, policy, diagnostics);
                if let Some(size) = &seg.size {
                    check_nested_guards(size, atom_names, policy, diagnostics);
                }
            }
        }
        ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            check_nested_guards(e, atom_names, policy, diagnostics);
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                check_nested_guards(&f.value, atom_names, policy, diagnostics);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            check_nested_guards(base, atom_names, policy, diagnostics);
            for f in fields {
                check_nested_guards(&f.value, atom_names, policy, diagnostics);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            check_nested_guards(base, atom_names, policy, diagnostics);
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => check_nested_guards(e, atom_names, policy, diagnostics),
                    ast::MaybeExpr::CondMatch(_, e, _) => {
                        check_nested_guards(e, atom_names, policy, diagnostics);
                    }
                }
            }
            for clause in else_clauses {
                if let Some(guard) = &clause.guard {
                    validate_guard(guard, atom_names, policy, diagnostics);
                }
                for e in &clause.body {
                    check_nested_guards(e, atom_names, policy, diagnostics);
                }
            }
        }
        // Leaf nodes: no nested guards possible.
        ast::Expr::Integer(..)
        | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(..)
        | ast::Expr::Atom(..)
        | ast::Expr::String(..)
        | ast::Expr::Nil(..)
        | ast::Expr::Variable(..)
        | ast::Expr::FunRef(..)
        | ast::Expr::RecordIndex { .. } => {}
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    check_nested_guards(arg, atom_names, policy, diagnostics);
                }
            }
        }
    }
}

/// Validate a guard node, dispatching to expression validation.
fn validate_guard(
    guard: &Guard,
    atom_names: &HashMap<AtomId, String>,
    policy: &crate::resolution::ResolutionPolicy,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match guard {
        Guard::Test(expr) => validate_guard_expr(expr, atom_names, policy, diagnostics),
        Guard::Conjunction(exprs) => {
            for expr in exprs {
                validate_guard_expr(expr, atom_names, policy, diagnostics);
            }
        }
        Guard::Disjunction(guards) => {
            for g in guards {
                validate_guard(g, atom_names, policy, diagnostics);
            }
        }
    }
}

/// Validate a single expression in guard position.
fn validate_guard_expr(
    expr: &ast::Expr,
    atom_names: &HashMap<AtomId, String>,
    policy: &crate::resolution::ResolutionPolicy,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match expr {
        // Always legal in guards:
        ast::Expr::Variable(..)
        | ast::Expr::Integer(..)
        | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(..)
        | ast::Expr::Atom(..)
        | ast::Expr::String(..)
        | ast::Expr::Nil(..) => {}

        // Binary/unary operators — recursively validate operands.
        ast::Expr::BinOp(op, lhs, rhs, span) => {
            // ListAppend and ListSubtract are not allowed in guards.
            match op {
                ast::BinOp::ListAppend | ast::BinOp::ListSubtract => {
                    emit_illegal_guard_expr(*span, diagnostics);
                }
                _ => {
                    validate_guard_expr(lhs, atom_names, policy, diagnostics);
                    validate_guard_expr(rhs, atom_names, policy, diagnostics);
                }
            }
        }
        ast::Expr::UnaryOp(_, operand, _) => {
            validate_guard_expr(operand, atom_names, policy, diagnostics);
        }

        // Tuple, cons, map construction — legal for comparison in guards.
        ast::Expr::Tuple(exprs, _) => {
            for e in exprs {
                validate_guard_expr(e, atom_names, policy, diagnostics);
            }
        }
        ast::Expr::Cons(hd, tl, _) => {
            validate_guard_expr(hd, atom_names, policy, diagnostics);
            validate_guard_expr(tl, atom_names, policy, diagnostics);
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                validate_guard_expr(&f.key, atom_names, policy, diagnostics);
                validate_guard_expr(&f.value, atom_names, policy, diagnostics);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            validate_guard_expr(base, atom_names, policy, diagnostics);
            for f in fields {
                validate_guard_expr(&f.key, atom_names, policy, diagnostics);
                validate_guard_expr(&f.value, atom_names, policy, diagnostics);
            }
        }

        // Record field access and index — legal in guards.
        ast::Expr::RecordAccess { base, .. } => {
            validate_guard_expr(base, atom_names, policy, diagnostics);
        }
        ast::Expr::RecordIndex { .. } => {}

        // Record create/update — legal in guards (record operations become tuple ops).
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                validate_guard_expr(&f.value, atom_names, policy, diagnostics);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            validate_guard_expr(base, atom_names, policy, diagnostics);
            for f in fields {
                validate_guard_expr(&f.value, atom_names, policy, diagnostics);
            }
        }

        // Binary construction — allowed since OTP 26.
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                validate_guard_expr(&seg.value, atom_names, policy, diagnostics);
                if let Some(size) = &seg.size {
                    validate_guard_expr(size, atom_names, policy, diagnostics);
                }
            }
        }

        // Function calls — must be guard BIFs.
        ast::Expr::Call(target, args, span) => {
            validate_guard_call(target, args, *span, atom_names, policy, diagnostics);
        }

        // Everything else is illegal in guards.
        ast::Expr::Match(_, _, span) => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::Send(_, _, span) => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::Case(_, _, span) => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::If(_, span) => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::Receive(_, _, span) => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::Block(_, span) => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::Fun(_, _, span) => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::FunRef(_, span) => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::Throw(_, span) => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::Catch(_, span) => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::ListComp(_, _, span) => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::BinComp(_, _, span) => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::Try { span, .. } => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::Maybe { span, .. } => emit_illegal_guard_expr(*span, diagnostics),
        ast::Expr::MacroCall { span, .. } => emit_illegal_guard_expr(*span, diagnostics),
    }
}

/// Validate a function call in guard position.
fn validate_guard_call(
    target: &CallTarget,
    args: &[ast::Expr],
    span: Span,
    atom_names: &HashMap<AtomId, String>,
    policy: &crate::resolution::ResolutionPolicy,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let arity = args.len() as u32;

    match target {
        CallTarget::Local(name_id, _) => {
            let name = match atom_names.get(name_id) {
                Some(s) => s.as_str(),
                None => {
                    // Atom name not found — can happen when atom_names is
                    // built from a different interner state (e.g. incremental
                    // parse cache in the LSP). Skip the check to avoid false
                    // positives.
                    return;
                }
            };

            // Check if this is a known guard BIF (auto-imported from erlang).
            let module = match policy.resolve(*name_id, arity) {
                crate::resolution::Resolution::AutoImport => Some("erlang"),
                crate::resolution::Resolution::Import(module) => atom_names.get(&module).map(String::as_str),
                _ => None,
            };
            if module.is_some_and(|module| is_guard_bif(module, name, arity)) {
                // Valid guard BIF — validate arguments recursively.
                for arg in args {
                    validate_guard_expr(arg, atom_names, policy, diagnostics);
                }
            } else {
                // Local function call — illegal in guard.
                diagnostics.push(
                    Diagnostic::error_at(
                        format!("call to local function '{}/{}' is illegal in guard", name, arity),
                        span,
                    )
                    .with_code("illegal_guard_local_call")
                    .with_hint(
                        "guards can only contain comparisons, type tests, arithmetic, and guard BIFs",
                    ),
                );
            }
        }
        CallTarget::Remote(mod_expr, fun_expr) => {
            // Only erlang:guard_bif calls are allowed.
            if let (ast::Expr::Atom(mod_id, _), ast::Expr::Atom(fun_id, _)) =
                (mod_expr.as_ref(), fun_expr.as_ref())
            {
                let mod_name = match atom_names.get(mod_id) {
                    Some(s) => s.as_str(),
                    None => return,
                };
                let fun_name = match atom_names.get(fun_id) {
                    Some(s) => s.as_str(),
                    None => return,
                };

                if is_guard_bif(mod_name, fun_name, arity) {
                    for arg in args {
                        validate_guard_expr(arg, atom_names, policy, diagnostics);
                    }
                } else {
                    diagnostics.push(
                        Diagnostic::error_at(
                            format!(
                                "call to '{}:{}/{}' is illegal in guard",
                                mod_name, fun_name, arity
                            ),
                            span,
                        )
                        .with_code("illegal_guard_expr")
                        .with_hint(
                            "guards can only contain comparisons, type tests, arithmetic, and guard BIFs",
                        ),
                    );
                }
            } else {
                // Dynamic remote call (Mod:Fun(Args)) — illegal in guard.
                emit_illegal_guard_expr(span, diagnostics);
            }
        }
        CallTarget::Variable(_, _) => {
            // Variable call (F(Args)) — illegal in guard.
            emit_illegal_guard_expr(span, diagnostics);
        }
        CallTarget::Expr(_) => {
            // Arbitrary-expression call (E)(Args) — illegal in guard.
            emit_illegal_guard_expr(span, diagnostics);
        }
    }
}

/// Check whether `module:function/arity` is a known guard BIF.
fn is_guard_bif(module: &str, function: &str, arity: u32) -> bool {
    GUARD_BIFS
        .iter()
        .any(|&(m, f, a)| m == module && f == function && a == arity)
}

/// Emit a generic "illegal guard expression" error.
fn emit_illegal_guard_expr(span: Span, diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.push(
        Diagnostic::error_at("illegal guard expression", span)
            .with_code("illegal_guard_expr")
            .with_hint(
                "guards can only contain comparisons, type tests, arithmetic, and guard BIFs",
            ),
    );
}
