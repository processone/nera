//! Undefined local function call detection.
//!
//! Checks that every local function call `foo(...)` and local fun reference
//! `fun foo/N` refers to a function that is actually defined in the module
//! (or is a BIF auto-import).  Reports an error when:
//! - The function name/arity pair does not exist at all.
//! - The function name exists but at a different arity (suggests the correct one).

use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;

use crate::resolution::{Resolution, ResolutionPolicy};

/// Check for calls and references to undefined local functions.
pub(super) fn check_undefined_functions(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    defined: &ResolutionPolicy,
    warnings: &mut Vec<Diagnostic>,
) {

    // Collect defined names (ignoring arity) for "did you mean" hints.
    let defined_arities: HashMap<AtomId, Vec<u32>> = {
        let mut map: HashMap<AtomId, Vec<u32>> = HashMap::new();
        for f in &module.functions {
            map.entry(f.name).or_default().push(f.arity);
        }
        map
    };

    // Track already-reported (name, arity) pairs to avoid duplicates.
    let mut reported: HashSet<(AtomId, u32)> = HashSet::new();

    for func in &module.functions {
        for clause in &func.clauses {
            if let Some(ref guard) = clause.guard {
                check_guard(guard, defined, &defined_arities, atom_names, &mut reported, warnings);
            }
            for expr in &clause.body {
                check_expr(expr, defined, &defined_arities, atom_names, &mut reported, true, warnings);
            }
        }
    }
}

#[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
fn check_call_target(
    name: AtomId,
    arity: u32,
    span: nera_syntax::span::Span,
    defined: &ResolutionPolicy,
    defined_arities: &HashMap<AtomId, Vec<u32>>,
    atom_names: &HashMap<AtomId, String>,
    reported: &mut HashSet<(AtomId, u32)>,
    warn_clash: bool,
    warnings: &mut Vec<Diagnostic>,
) {
    let resolved = defined.resolve(name, arity);
    if check_compiler_form(resolved, name, arity, span, atom_names, warnings) {
        return;
    }
    match resolved {
        Resolution::Local => {
            if warn_clash && defined.auto_enabled(name, arity) {
                let display = atom_names.get(&name).map(String::as_str).unwrap_or("?");
                warnings.push(Diagnostic::warning_at(
                    format!("ambiguous call to overridden auto-imported BIF {display}/{arity}"), span,
                ).with_code("bif_clash").with_hint("use erlang:Function(...) or -compile({no_auto_import, [Function/Arity]})"));
            }
            return;
        }
        Resolution::Import(_) | Resolution::AutoImport => return,
        Resolution::Undefined => {}
    }

    // Avoid duplicate reports for the same (name, arity).
    if !reported.insert((name, arity)) {
        return;
    }

    let display_name = atom_names
        .get(&name)
        .map(|s| s.as_str())
        .unwrap_or("?");

    if let Some(arities) = defined_arities.get(&name) {
        // Function name exists but at a different arity.
        let available: Vec<String> = arities.iter().map(|a| format!("{}/{}", display_name, a)).collect();
        let hint = if available.len() == 1 {
            format!("did you mean '{}'?", available[0])
        } else {
            format!("available arities: {}", available.join(", "))
        };
        warnings.push(
            Diagnostic::error_at(
                format!("function '{}/{}' is undefined", display_name, arity),
                span,
            )
            .with_code("undefined_function")
            .with_hint(hint),
        );
    } else {
        warnings.push(
            Diagnostic::error_at(
                format!("function '{}/{}' is undefined", display_name, arity),
                span,
            )
            .with_code("undefined_function"),
        );
    }
}

/// Reject compiler-only forms before guard lowering can turn errors into false.
fn check_compiler_form(
    target: Resolution,
    name: AtomId,
    arity: u32,
    span: nera_syntax::span::Span,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    if !ResolutionPolicy::is_compiler_form(target, name, arity, atom_names) {
        return false;
    }
    let name = atom_names.get(&name).map(String::as_str).unwrap_or("");
    diagnostics.push(Diagnostic::error_at(
        format!("compiler-specific {name}/{arity} is not supported"), span,
    ).with_code("unsupported_compiler_form"));
    true
}

fn check_expr(
    expr: &ast::Expr,
    defined: &ResolutionPolicy,
    defined_arities: &HashMap<AtomId, Vec<u32>>,
    atom_names: &HashMap<AtomId, String>,
    reported: &mut HashSet<(AtomId, u32)>,
    warn_clash: bool,
    warnings: &mut Vec<Diagnostic>,
) {
    match expr {
        ast::Expr::Call(target, args, span) => {
            if let ast::CallTarget::Local(name, span) = target {
                check_call_target(
                    *name, args.len() as u32, *span,
                    defined, defined_arities, atom_names, reported, warn_clash, warnings,
                );
            }
            // Recurse into all sub-expressions (including remote module/fun exprs).
            if let ast::CallTarget::Remote(module_expr, fun_expr) = target {
                if let (ast::Expr::Atom(module, _), ast::Expr::Atom(name, _)) = (module_expr.as_ref(), fun_expr.as_ref()) {
                    check_compiler_form(Resolution::Import(*module), *name, args.len() as u32, *span, atom_names, warnings);
                }
                check_expr(module_expr, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                check_expr(fun_expr, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            }
            if let ast::CallTarget::Expr(callee) = target {
                check_expr(callee, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            }
            for arg in args {
                check_expr(arg, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            }
        }
        ast::Expr::FunRef(ast::FunRefTarget::Local(name, arity), span) => {
            check_call_target(
                *name, *arity, *span,
                defined, defined_arities, atom_names, reported, warn_clash, warnings,
            );
        }
        ast::Expr::FunRef(ast::FunRefTarget::Remote(module, name, arity), span) => {
            check_compiler_form(Resolution::Import(*module), *name, *arity, *span, atom_names, warnings);
        }
        // Recurse into sub-expressions.
        ast::Expr::Tuple(exprs, _) | ast::Expr::Block(exprs, _) => {
            for e in exprs {
                check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) | ast::Expr::BinOp(_, h, t, _) => {
            check_expr(h, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            check_expr(t, defined, defined_arities, atom_names, reported, warn_clash, warnings);
        }
        ast::Expr::Match(_, e, _) | ast::Expr::UnaryOp(_, e, _)
        | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings);
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            check_expr(scrutinee, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            for clause in clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, defined, defined_arities, atom_names, reported, warnings);
                }
                for e in &clause.body {
                    check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                }
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                check_guard(&clause.guard, defined, defined_arities, atom_names, reported, warnings);
                for e in &clause.body {
                    check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                }
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, defined, defined_arities, atom_names, reported, warnings);
                }
                for e in &clause.body {
                    check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                }
            }
            if let Some(after) = after {
                check_expr(&after.timeout, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                for e in &after.body {
                    check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                }
            }
        }
        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            for e in body {
                check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            }
            for clause in of_clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, defined, defined_arities, atom_names, reported, warnings);
                }
                for e in &clause.body {
                    check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                }
            }
            for clause in catch_clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, defined, defined_arities, atom_names, reported, warnings);
                }
                for e in &clause.body {
                    check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                }
            }
            if let Some(after) = after_body {
                for e in after {
                    check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                }
            }
        }
        ast::Expr::Fun(_name, clauses, _) => {
            for clause in clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, defined, defined_arities, atom_names, reported, warnings);
                }
                for e in &clause.body {
                    check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                }
            }
        }
        ast::Expr::ListComp(body, qualifiers, _) | ast::Expr::BinComp(body, qualifiers, _) => {
            check_expr(body, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(_, e, _)
                    | ast::LcQualifier::BinGenerator(_, e, _)
                    | ast::LcQualifier::Filter(e, _) => {
                        check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                    }
                }
            }
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                check_expr(&seg.value, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                if let Some(ref size) = seg.size {
                    check_expr(size, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                check_expr(&f.key, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                check_expr(&f.value, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_expr(base, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            for f in fields {
                check_expr(&f.key, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                check_expr(&f.value, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            }
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                check_expr(&f.value, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            check_expr(base, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            for f in fields {
                check_expr(&f.value, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            check_expr(base, defined, defined_arities, atom_names, reported, warn_clash, warnings);
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings),
                    ast::MaybeExpr::CondMatch(_, e, _) => check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings),
                }
            }
            for clause in else_clauses {
                if let Some(guard) = &clause.guard {
                    check_guard(guard, defined, defined_arities, atom_names, reported, warnings);
                }
                for e in &clause.body {
                    check_expr(e, defined, defined_arities, atom_names, reported, warn_clash, warnings);
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
        | ast::Expr::RecordIndex { .. } => {}
        ast::Expr::FunRef(ast::FunRefTarget::DynamicRemote(m, f, a), _) => {
            check_expr(m, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            check_expr(f, defined, defined_arities, atom_names, reported, warn_clash, warnings);
            check_expr(a, defined, defined_arities, atom_names, reported, warn_clash, warnings);
        }
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    check_expr(arg, defined, defined_arities, atom_names, reported, warn_clash, warnings);
                }
            }
        }
    }
}

fn check_guard(
    guard: &ast::Guard,
    defined: &ResolutionPolicy,
    defined_arities: &HashMap<AtomId, Vec<u32>>,
    atom_names: &HashMap<AtomId, String>,
    reported: &mut HashSet<(AtomId, u32)>,
    warnings: &mut Vec<Diagnostic>,
) {
    match guard {
        ast::Guard::Test(e) => check_expr(e, defined, defined_arities, atom_names, reported, false, warnings),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                check_expr(e, defined, defined_arities, atom_names, reported, false, warnings);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                check_guard(g, defined, defined_arities, atom_names, reported, warnings);
            }
        }
    }
}
