//! Unused function detection.
//!
//! A function that is defined in a module but is neither exported nor
//! called by any other function in the same module is dead code.

use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;
use nera_syntax::span::Span;

/// Check for functions that are neither exported nor called locally.
pub(super) fn check_unused_functions(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    // 1. Build set of exported function signatures.
    let exported: HashSet<(AtomId, u32)> = module
        .exports
        .iter()
        .map(|fa| (fa.name, fa.arity))
        .collect();

    // 2. Build set of all defined function signatures.
    let defined: HashMap<(AtomId, u32), Span> = module
        .functions
        .iter()
        .map(|f| ((f.name, f.arity), f.span))
        .collect();

    // 3. Collect all local call targets across all function bodies.
    let mut called: HashSet<(AtomId, u32)> = HashSet::new();
    for func in &module.functions {
        for clause in &func.clauses {
            if let Some(ref guard) = clause.guard {
                collect_calls_in_guard(guard, &mut called);
            }
            for expr in &clause.body {
                collect_calls_in_expr(expr, &mut called);
            }
        }
    }

    // 4. Emit warnings for defined functions that are neither exported
    //    nor called locally.
    for ((name, arity), span) in &defined {
        if exported.contains(&(*name, *arity)) {
            continue;
        }
        if called.contains(&(*name, *arity)) {
            continue;
        }
        let func_name = atom_names
            .get(name)
            .map(|s| s.as_str())
            .unwrap_or("?");
        warnings.push(Diagnostic::warning_at(
            format!("function '{}/{}' is unused", func_name, arity),
            *span,
        ).with_code("unused_function"));
    }
}

/// Collect local call targets (name, arity) from an expression tree.
fn collect_calls_in_expr(expr: &ast::Expr, calls: &mut HashSet<(AtomId, u32)>) {
    match expr {
        ast::Expr::Call(target, args, _) => {
            match target {
                ast::CallTarget::Local(name, _) => {
                    calls.insert((*name, args.len() as u32));
                }
                ast::CallTarget::Remote(module_expr, fun_expr) => {
                    collect_calls_in_expr(module_expr, calls);
                    collect_calls_in_expr(fun_expr, calls);
                }
                ast::CallTarget::Variable(_, _) => {}
                ast::CallTarget::Expr(callee) => {
                    collect_calls_in_expr(callee, calls);
                }
            }
            for arg in args {
                collect_calls_in_expr(arg, calls);
            }
        }
        ast::Expr::FunRef(ast::FunRefTarget::Local(name, arity), _) => {
            calls.insert((*name, *arity));
        }
        // Recurse into sub-expressions
        ast::Expr::Tuple(exprs, _) | ast::Expr::Block(exprs, _) => {
            for e in exprs {
                collect_calls_in_expr(e, calls);
            }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) | ast::Expr::BinOp(_, h, t, _) => {
            collect_calls_in_expr(h, calls);
            collect_calls_in_expr(t, calls);
        }
        ast::Expr::Match(_, e, _) | ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            collect_calls_in_expr(e, calls);
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            collect_calls_in_expr(scrutinee, calls);
            for clause in clauses {
                if let Some(ref guard) = clause.guard {
                    collect_calls_in_guard(guard, calls);
                }
                for e in &clause.body {
                    collect_calls_in_expr(e, calls);
                }
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                collect_calls_in_guard(&clause.guard, calls);
                for e in &clause.body {
                    collect_calls_in_expr(e, calls);
                }
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                if let Some(ref guard) = clause.guard {
                    collect_calls_in_guard(guard, calls);
                }
                for e in &clause.body {
                    collect_calls_in_expr(e, calls);
                }
            }
            if let Some(after) = after {
                collect_calls_in_expr(&after.timeout, calls);
                for e in &after.body {
                    collect_calls_in_expr(e, calls);
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
                collect_calls_in_expr(e, calls);
            }
            for clause in of_clauses {
                if let Some(ref guard) = clause.guard {
                    collect_calls_in_guard(guard, calls);
                }
                for e in &clause.body {
                    collect_calls_in_expr(e, calls);
                }
            }
            for clause in catch_clauses {
                if let Some(ref guard) = clause.guard {
                    collect_calls_in_guard(guard, calls);
                }
                for e in &clause.body {
                    collect_calls_in_expr(e, calls);
                }
            }
            if let Some(after) = after_body {
                for e in after {
                    collect_calls_in_expr(e, calls);
                }
            }
        }
        ast::Expr::Fun(_name, clauses, _) => {
            for clause in clauses {
                if let Some(ref guard) = clause.guard {
                    collect_calls_in_guard(guard, calls);
                }
                for e in &clause.body {
                    collect_calls_in_expr(e, calls);
                }
            }
        }
        ast::Expr::ListComp(body, qualifiers, _) | ast::Expr::BinComp(body, qualifiers, _) => {
            collect_calls_in_expr(body, calls);
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(_, e, _)
                    | ast::LcQualifier::BinGenerator(_, e, _)
                    | ast::LcQualifier::Filter(e, _) => {
                        collect_calls_in_expr(e, calls);
                    }
                }
            }
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                collect_calls_in_expr(&seg.value, calls);
                if let Some(ref size) = seg.size {
                    collect_calls_in_expr(size, calls);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                collect_calls_in_expr(&f.key, calls);
                collect_calls_in_expr(&f.value, calls);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            collect_calls_in_expr(base, calls);
            for f in fields {
                collect_calls_in_expr(&f.key, calls);
                collect_calls_in_expr(&f.value, calls);
            }
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                collect_calls_in_expr(&f.value, calls);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            collect_calls_in_expr(base, calls);
            for f in fields {
                collect_calls_in_expr(&f.value, calls);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            collect_calls_in_expr(base, calls);
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => collect_calls_in_expr(e, calls),
                    ast::MaybeExpr::CondMatch(_, e, _) => collect_calls_in_expr(e, calls),
                }
            }
            for clause in else_clauses {
                for e in &clause.body {
                    collect_calls_in_expr(e, calls);
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
            collect_calls_in_expr(m, calls);
            collect_calls_in_expr(f, calls);
            collect_calls_in_expr(a, calls);
        }
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    collect_calls_in_expr(arg, calls);
                }
            }
        }
    }
}

/// Collect call targets from guard expressions.
fn collect_calls_in_guard(guard: &ast::Guard, calls: &mut HashSet<(AtomId, u32)>) {
    match guard {
        ast::Guard::Test(e) => collect_calls_in_expr(e, calls),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                collect_calls_in_expr(e, calls);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                collect_calls_in_guard(g, calls);
            }
        }
    }
}
