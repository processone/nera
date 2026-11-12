//! Undefined record field detection.
//!
//! Checks that every field name used in record expressions and patterns
//! actually exists in the corresponding record definition.  Reports an
//! error when a field name is not found, with a hint listing the available
//! fields.  Skips records that are themselves undefined (that error is
//! reported separately during IR lowering).

use std::collections::HashMap;

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;

/// Check for uses of undefined fields in record expressions and patterns.
pub(super) fn check_undefined_record_fields(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Build a lookup from record name → definition.
    let record_defs: HashMap<AtomId, &ast::RecordDef> = module
        .records
        .iter()
        .map(|r| (r.name, r))
        .collect();

    if record_defs.is_empty() {
        return;
    }

    for func in &module.functions {
        for clause in &func.clauses {
            for pat in &clause.patterns {
                check_pattern(pat, &record_defs, atom_names, diagnostics);
            }
            if let Some(ref guard) = clause.guard {
                check_guard(guard, &record_defs, atom_names, diagnostics);
            }
            for expr in &clause.body {
                check_expr(expr, &record_defs, atom_names, diagnostics);
            }
        }
    }
}

/// Check whether `field_name` exists in `rec_def`.  If not, push an error.
fn check_field(
    rec_def: &ast::RecordDef,
    field_name: AtomId,
    field_span: nera_syntax::span::Span,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if rec_def.fields.iter().any(|f| f.name == field_name) {
        return;
    }

    let fname = atom_names
        .get(&field_name)
        .map(|s| s.as_str())
        .unwrap_or("?");
    let rname = atom_names
        .get(&rec_def.name)
        .map(|s| s.as_str())
        .unwrap_or("?");

    let available: Vec<&str> = rec_def
        .fields
        .iter()
        .filter_map(|f| atom_names.get(&f.name).map(|s| s.as_str()))
        .collect();

    let mut diag = Diagnostic::error_at(
        format!("field '{}' is undefined in record '{}'", fname, rname),
        field_span,
    )
    .with_code("undefined_record_field");

    if !available.is_empty() {
        diag = diag.with_hint(format!("available fields: {}", available.join(", ")));
    }

    diagnostics.push(diag);
}

/// Check record field references in an expression.
fn check_expr(
    expr: &ast::Expr,
    record_defs: &HashMap<AtomId, &ast::RecordDef>,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match expr {
        ast::Expr::RecordCreate { record_name, fields, .. } => {
            if let Some(rec_def) = record_defs.get(record_name) {
                for f in fields {
                    check_field(rec_def, f.field_name, f.field_name_span, atom_names, diagnostics);
                }
            }
            // Recurse into field value expressions.
            for f in fields {
                check_expr(&f.value, record_defs, atom_names, diagnostics);
            }
        }
        ast::Expr::RecordUpdate { base, record_name, fields, .. } => {
            if let Some(rec_def) = record_defs.get(record_name) {
                for f in fields {
                    check_field(rec_def, f.field_name, f.field_name_span, atom_names, diagnostics);
                }
            }
            check_expr(base, record_defs, atom_names, diagnostics);
            for f in fields {
                check_expr(&f.value, record_defs, atom_names, diagnostics);
            }
        }
        ast::Expr::RecordAccess { base, record_name, field_name, field_name_span, .. } => {
            if let Some(rec_def) = record_defs.get(record_name) {
                check_field(rec_def, *field_name, *field_name_span, atom_names, diagnostics);
            }
            check_expr(base, record_defs, atom_names, diagnostics);
        }
        ast::Expr::RecordIndex { record_name, field_name, field_name_span, .. } => {
            if let Some(rec_def) = record_defs.get(record_name) {
                check_field(rec_def, *field_name, *field_name_span, atom_names, diagnostics);
            }
        }
        // Recurse into sub-expressions.
        ast::Expr::Call(target, args, _) => {
            if let ast::CallTarget::Remote(module_expr, fun_expr) = target {
                check_expr(module_expr, record_defs, atom_names, diagnostics);
                check_expr(fun_expr, record_defs, atom_names, diagnostics);
            }
            for arg in args {
                check_expr(arg, record_defs, atom_names, diagnostics);
            }
        }
        ast::Expr::Tuple(exprs, _) | ast::Expr::Block(exprs, _) => {
            for e in exprs {
                check_expr(e, record_defs, atom_names, diagnostics);
            }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) | ast::Expr::BinOp(_, h, t, _) => {
            check_expr(h, record_defs, atom_names, diagnostics);
            check_expr(t, record_defs, atom_names, diagnostics);
        }
        ast::Expr::Match(_, e, _) | ast::Expr::UnaryOp(_, e, _)
        | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            check_expr(e, record_defs, atom_names, diagnostics);
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            check_expr(scrutinee, record_defs, atom_names, diagnostics);
            for clause in clauses {
                check_pattern(&clause.pattern, record_defs, atom_names, diagnostics);
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, record_defs, atom_names, diagnostics);
                }
                for e in &clause.body {
                    check_expr(e, record_defs, atom_names, diagnostics);
                }
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                check_guard(&clause.guard, record_defs, atom_names, diagnostics);
                for e in &clause.body {
                    check_expr(e, record_defs, atom_names, diagnostics);
                }
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                check_pattern(&clause.pattern, record_defs, atom_names, diagnostics);
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, record_defs, atom_names, diagnostics);
                }
                for e in &clause.body {
                    check_expr(e, record_defs, atom_names, diagnostics);
                }
            }
            if let Some(after) = after {
                check_expr(&after.timeout, record_defs, atom_names, diagnostics);
                for e in &after.body {
                    check_expr(e, record_defs, atom_names, diagnostics);
                }
            }
        }
        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            for e in body {
                check_expr(e, record_defs, atom_names, diagnostics);
            }
            for clause in of_clauses {
                check_pattern(&clause.pattern, record_defs, atom_names, diagnostics);
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, record_defs, atom_names, diagnostics);
                }
                for e in &clause.body {
                    check_expr(e, record_defs, atom_names, diagnostics);
                }
            }
            for clause in catch_clauses {
                check_pattern(&clause.pattern, record_defs, atom_names, diagnostics);
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, record_defs, atom_names, diagnostics);
                }
                for e in &clause.body {
                    check_expr(e, record_defs, atom_names, diagnostics);
                }
            }
            if let Some(after) = after_body {
                for e in after {
                    check_expr(e, record_defs, atom_names, diagnostics);
                }
            }
        }
        ast::Expr::Fun(_name, clauses, _) => {
            for clause in clauses {
                for pat in &clause.patterns {
                    check_pattern(pat, record_defs, atom_names, diagnostics);
                }
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, record_defs, atom_names, diagnostics);
                }
                for e in &clause.body {
                    check_expr(e, record_defs, atom_names, diagnostics);
                }
            }
        }
        ast::Expr::ListComp(body, qualifiers, _) | ast::Expr::BinComp(body, qualifiers, _) => {
            check_expr(body, record_defs, atom_names, diagnostics);
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(pat, e, _) | ast::LcQualifier::BinGenerator(pat, e, _) => {
                        check_pattern(pat, record_defs, atom_names, diagnostics);
                        check_expr(e, record_defs, atom_names, diagnostics);
                    }
                    ast::LcQualifier::Filter(e, _) => {
                        check_expr(e, record_defs, atom_names, diagnostics);
                    }
                }
            }
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                check_expr(&seg.value, record_defs, atom_names, diagnostics);
                if let Some(ref size) = seg.size {
                    check_expr(size, record_defs, atom_names, diagnostics);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                check_expr(&f.key, record_defs, atom_names, diagnostics);
                check_expr(&f.value, record_defs, atom_names, diagnostics);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_expr(base, record_defs, atom_names, diagnostics);
            for f in fields {
                check_expr(&f.key, record_defs, atom_names, diagnostics);
                check_expr(&f.value, record_defs, atom_names, diagnostics);
            }
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => check_expr(e, record_defs, atom_names, diagnostics),
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        check_pattern(pat, record_defs, atom_names, diagnostics);
                        check_expr(e, record_defs, atom_names, diagnostics);
                    }
                }
            }
            for clause in else_clauses {
                check_pattern(&clause.pattern, record_defs, atom_names, diagnostics);
                for e in &clause.body {
                    check_expr(e, record_defs, atom_names, diagnostics);
                }
            }
        }
        // Leaf nodes — no sub-expressions to check.
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
                    check_expr(arg, record_defs, atom_names, diagnostics);
                }
            }
        }
    }
}

/// Check record field references in a pattern.
fn check_pattern(
    pat: &ast::Pattern,
    record_defs: &HashMap<AtomId, &ast::RecordDef>,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match pat {
        ast::Pattern::Record { record_name, fields, .. } => {
            if let Some(rec_def) = record_defs.get(record_name) {
                for f in fields {
                    check_field(rec_def, f.field_name, f.field_name_span, atom_names, diagnostics);
                }
            }
            for f in fields {
                check_pattern(&f.pattern, record_defs, atom_names, diagnostics);
            }
        }
        ast::Pattern::Tuple(pats, _) => {
            for p in pats {
                check_pattern(p, record_defs, atom_names, diagnostics);
            }
        }
        ast::Pattern::Cons(h, t, _) => {
            check_pattern(h, record_defs, atom_names, diagnostics);
            check_pattern(t, record_defs, atom_names, diagnostics);
        }
        ast::Pattern::Match(p1, p2, _) => {
            check_pattern(p1, record_defs, atom_names, diagnostics);
            check_pattern(p2, record_defs, atom_names, diagnostics);
        }
        ast::Pattern::Binary(segments, _) => {
            for seg in segments {
                check_pattern(&seg.value, record_defs, atom_names, diagnostics);
            }
        }
        ast::Pattern::Map(fields, _) => {
            for field in fields {
                check_pattern(&field.value, record_defs, atom_names, diagnostics);
            }
        }
        ast::Pattern::Concat(lhs, rhs, _) => {
            check_pattern(lhs, record_defs, atom_names, diagnostics);
            check_pattern(rhs, record_defs, atom_names, diagnostics);
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
                    check_pattern(arg, record_defs, atom_names, diagnostics);
                }
            }
        }
    }
}

/// Check record field references in guard expressions.
fn check_guard(
    guard: &ast::Guard,
    record_defs: &HashMap<AtomId, &ast::RecordDef>,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match guard {
        ast::Guard::Test(e) => check_expr(e, record_defs, atom_names, diagnostics),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                check_expr(e, record_defs, atom_names, diagnostics);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                check_guard(g, record_defs, atom_names, diagnostics);
            }
        }
    }
}
