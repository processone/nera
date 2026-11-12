//! Unused record detection.
//!
//! A record that is defined in a module via `-record(name, {...})` but is
//! never referenced anywhere in the module is dead code.

use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;

/// Collect the set of record names that are actually used (referenced) in the
/// module's function bodies, patterns, and guards.  Returns string names so
/// the caller can aggregate across modules without sharing an atom table.
pub fn collect_used_record_names(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
) -> HashSet<String> {
    let used_ids = collect_used_record_ids(module);
    used_ids
        .into_iter()
        .filter_map(|id| atom_names.get(&id).cloned())
        .collect()
}

/// Internal: collect used record AtomIds from a module.
fn collect_used_record_ids(module: &ast::Module) -> HashSet<AtomId> {
    let mut used: HashSet<AtomId> = HashSet::new();
    for func in &module.functions {
        for clause in &func.clauses {
            for pat in &clause.patterns {
                collect_record_refs_in_pattern(pat, &mut used);
            }
            if let Some(ref guard) = clause.guard {
                collect_record_refs_in_guard(guard, &mut used);
            }
            for expr in &clause.body {
                collect_record_refs_in_expr(expr, &mut used);
            }
        }
    }
    used
}

/// Check for records that are defined but never used.
pub(super) fn check_unused_records(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    if module.records.is_empty() {
        return;
    }

    // 1. Collect all used record names across function bodies, patterns, and guards.
    let used = collect_used_record_ids(module);

    // 2. Emit warnings for defined records that are never used.
    for rec in &module.records {
        if !used.contains(&rec.name) {
            let name = atom_names
                .get(&rec.name)
                .map(|s| s.as_str())
                .unwrap_or("?");
            warnings.push(
                Diagnostic::warning_at(
                    format!("record '{}' is unused", name),
                    rec.name_span,
                )
                .with_code("unused_record"),
            );
        }
    }
}

/// Collect record name references from an expression tree.
fn collect_record_refs_in_expr(expr: &ast::Expr, used: &mut HashSet<AtomId>) {
    match expr {
        ast::Expr::RecordCreate { record_name, fields, .. } => {
            used.insert(*record_name);
            for f in fields {
                collect_record_refs_in_expr(&f.value, used);
            }
        }
        ast::Expr::RecordUpdate { base, record_name, fields, .. } => {
            used.insert(*record_name);
            collect_record_refs_in_expr(base, used);
            for f in fields {
                collect_record_refs_in_expr(&f.value, used);
            }
        }
        ast::Expr::RecordAccess { base, record_name, .. } => {
            used.insert(*record_name);
            collect_record_refs_in_expr(base, used);
        }
        ast::Expr::RecordIndex { record_name, .. } => {
            used.insert(*record_name);
        }
        // Recurse into sub-expressions
        ast::Expr::Call(target, args, _) => {
            if let ast::CallTarget::Remote(module_expr, fun_expr) = target {
                collect_record_refs_in_expr(module_expr, used);
                collect_record_refs_in_expr(fun_expr, used);
            }
            for arg in args {
                collect_record_refs_in_expr(arg, used);
            }
        }
        ast::Expr::Tuple(exprs, _) | ast::Expr::Block(exprs, _) => {
            for e in exprs {
                collect_record_refs_in_expr(e, used);
            }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) | ast::Expr::BinOp(_, h, t, _) => {
            collect_record_refs_in_expr(h, used);
            collect_record_refs_in_expr(t, used);
        }
        ast::Expr::Match(_, e, _) | ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            collect_record_refs_in_expr(e, used);
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            collect_record_refs_in_expr(scrutinee, used);
            for clause in clauses {
                collect_record_refs_in_pattern(&clause.pattern, used);
                if let Some(ref guard) = clause.guard {
                    collect_record_refs_in_guard(guard, used);
                }
                for e in &clause.body {
                    collect_record_refs_in_expr(e, used);
                }
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                collect_record_refs_in_guard(&clause.guard, used);
                for e in &clause.body {
                    collect_record_refs_in_expr(e, used);
                }
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                collect_record_refs_in_pattern(&clause.pattern, used);
                if let Some(ref guard) = clause.guard {
                    collect_record_refs_in_guard(guard, used);
                }
                for e in &clause.body {
                    collect_record_refs_in_expr(e, used);
                }
            }
            if let Some(after) = after {
                collect_record_refs_in_expr(&after.timeout, used);
                for e in &after.body {
                    collect_record_refs_in_expr(e, used);
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
                collect_record_refs_in_expr(e, used);
            }
            for clause in of_clauses {
                collect_record_refs_in_pattern(&clause.pattern, used);
                if let Some(ref guard) = clause.guard {
                    collect_record_refs_in_guard(guard, used);
                }
                for e in &clause.body {
                    collect_record_refs_in_expr(e, used);
                }
            }
            for clause in catch_clauses {
                collect_record_refs_in_pattern(&clause.pattern, used);
                if let Some(ref guard) = clause.guard {
                    collect_record_refs_in_guard(guard, used);
                }
                for e in &clause.body {
                    collect_record_refs_in_expr(e, used);
                }
            }
            if let Some(after) = after_body {
                for e in after {
                    collect_record_refs_in_expr(e, used);
                }
            }
        }
        ast::Expr::Fun(_name, clauses, _) => {
            for clause in clauses {
                for pat in &clause.patterns {
                    collect_record_refs_in_pattern(pat, used);
                }
                if let Some(ref guard) = clause.guard {
                    collect_record_refs_in_guard(guard, used);
                }
                for e in &clause.body {
                    collect_record_refs_in_expr(e, used);
                }
            }
        }
        ast::Expr::ListComp(body, qualifiers, _) | ast::Expr::BinComp(body, qualifiers, _) => {
            collect_record_refs_in_expr(body, used);
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(pat, e, _) | ast::LcQualifier::BinGenerator(pat, e, _) => {
                        collect_record_refs_in_pattern(pat, used);
                        collect_record_refs_in_expr(e, used);
                    }
                    ast::LcQualifier::Filter(e, _) => {
                        collect_record_refs_in_expr(e, used);
                    }
                }
            }
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                collect_record_refs_in_expr(&seg.value, used);
                if let Some(ref size) = seg.size {
                    collect_record_refs_in_expr(size, used);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                collect_record_refs_in_expr(&f.key, used);
                collect_record_refs_in_expr(&f.value, used);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            collect_record_refs_in_expr(base, used);
            for f in fields {
                collect_record_refs_in_expr(&f.key, used);
                collect_record_refs_in_expr(&f.value, used);
            }
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => collect_record_refs_in_expr(e, used),
                    ast::MaybeExpr::CondMatch(_, e, _) => collect_record_refs_in_expr(e, used),
                }
            }
            for clause in else_clauses {
                collect_record_refs_in_pattern(&clause.pattern, used);
                for e in &clause.body {
                    collect_record_refs_in_expr(e, used);
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
        | ast::Expr::FunRef(_, _) => {}
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    collect_record_refs_in_expr(arg, used);
                }
            }
        }
    }
}

/// Collect record name references from a pattern.
fn collect_record_refs_in_pattern(pat: &ast::Pattern, used: &mut HashSet<AtomId>) {
    match pat {
        ast::Pattern::Record { record_name, fields, .. } => {
            used.insert(*record_name);
            for field in fields {
                collect_record_refs_in_pattern(&field.pattern, used);
            }
        }
        ast::Pattern::Tuple(pats, _) => {
            for p in pats {
                collect_record_refs_in_pattern(p, used);
            }
        }
        ast::Pattern::Cons(h, t, _) => {
            collect_record_refs_in_pattern(h, used);
            collect_record_refs_in_pattern(t, used);
        }
        ast::Pattern::Match(p1, p2, _) => {
            collect_record_refs_in_pattern(p1, used);
            collect_record_refs_in_pattern(p2, used);
        }
        ast::Pattern::Binary(segments, _) => {
            for seg in segments {
                collect_record_refs_in_pattern(&seg.value, used);
            }
        }
        ast::Pattern::Map(fields, _) => {
            for field in fields {
                collect_record_refs_in_pattern(&field.value, used);
            }
        }
        ast::Pattern::Concat(lhs, rhs, _) => {
            collect_record_refs_in_pattern(lhs, used);
            collect_record_refs_in_pattern(rhs, used);
        }
        // Leaf patterns
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
                    collect_record_refs_in_pattern(arg, used);
                }
            }
        }
    }
}

/// Collect record name references from guard expressions.
fn collect_record_refs_in_guard(guard: &ast::Guard, used: &mut HashSet<AtomId>) {
    match guard {
        ast::Guard::Test(e) => collect_record_refs_in_expr(e, used),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                collect_record_refs_in_expr(e, used);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                collect_record_refs_in_guard(g, used);
            }
        }
    }
}
