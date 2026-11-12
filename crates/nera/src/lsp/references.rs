use std::sync::{Arc, Mutex};

use tower_lsp::lsp_types::*;

use nera_ir::bif_table;
use nera_syntax::ast;
use nera_syntax::source_map::SourceMap;
use nera_syntax::span::Span;

use super::types::ParsedModuleCache;
use super::util::{span_text, pattern_span, expr_span, in_span};
use super::source_map::span_to_lsp_range;

// ── Find all references helpers ────────────────────────────────────────

/// Find all call sites of a local function (name + arity) within a module's AST.
pub(crate) fn find_local_call_spans(module: &ast::Module, source: &str, name: &str, arity: u32) -> Vec<Span> {
    let mut spans = Vec::new();
    for func in &module.functions {
        for clause in &func.clauses {
            collect_call_spans_in_exprs(&clause.body, source, None, name, arity, &mut spans);
            if let Some(guard) = &clause.guard {
                collect_call_spans_in_guard(guard, source, None, name, arity, &mut spans);
            }
        }
    }
    spans
}

/// Find all call sites of a remote function (module:name/arity) within a module's AST.
pub(crate) fn find_remote_call_spans(module: &ast::Module, source: &str, target_mod: &str, name: &str, arity: u32) -> Vec<Span> {
    let mut spans = Vec::new();
    for func in &module.functions {
        for clause in &func.clauses {
            collect_call_spans_in_exprs(&clause.body, source, Some(target_mod), name, arity, &mut spans);
            if let Some(guard) = &clause.guard {
                collect_call_spans_in_guard(guard, source, Some(target_mod), name, arity, &mut spans);
            }
        }
    }
    spans
}

/// Target of a function rename operation: the module that owns the function
/// plus its name/arity.
#[derive(Debug, Clone)]
pub(crate) struct FunctionRenameTarget {
    pub(crate) owner_module: String,
    pub(crate) name: String,
    pub(crate) arity: u32,
}

/// Extract the current module name from its `-module(Name).` attribute span.
pub(crate) fn module_name_of(module: &ast::Module, source: &str) -> String {
    let text = span_text(source, module.name_span);
    let end = text.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '@').unwrap_or(text.len());
    text[..end].to_string()
}

/// Find the exact name-span and name at the cursor when `offset` falls on a
/// renameable function occurrence (definition, local/remote call, local/remote
/// fun ref, export entry, or spec name).
pub(crate) fn function_rename_span_at(
    module: &ast::Module,
    source: &str,
    offset: usize,
) -> Option<(Span, String)> {
    // Function definition name (and each clause's name_span).
    for func in &module.functions {
        if in_span(offset, func.name_span) {
            let name = span_text(source, func.name_span).to_string();
            return Some((func.name_span, name));
        }
        for clause in &func.clauses {
            if in_span(offset, clause.name_span) {
                let name = span_text(source, clause.name_span).to_string();
                return Some((clause.name_span, name));
            }
        }
    }
    // Export list entries: `-export([name/arity, ...]).`
    for fa in &module.exports {
        if in_span(offset, fa.span) {
            let text = span_text(source, fa.span);
            let end = text.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '@').unwrap_or(text.len());
            let name = text[..end].to_string();
            let span = Span { start: fa.span.start, end: fa.span.start + end };
            return Some((span, name));
        }
    }
    // Spec / callback function names: `-spec name(...) -> ...`.
    for spec in &module.specs {
        if in_span(offset, spec.name_span) {
            let name = span_text(source, spec.name_span).to_string();
            return Some((spec.name_span, name));
        }
    }
    // Local calls / fun refs and remote calls / fun refs inside clauses.
    for func in &module.functions {
        if !in_span(offset, func.span) { continue; }
        for clause in &func.clauses {
            if !in_span(offset, clause.span) { continue; }
            if let Some(hit) = find_func_span_in_exprs(&clause.body, source, offset) {
                return Some(hit);
            }
            if let Some(guard) = &clause.guard {
                if let Some(hit) = find_func_span_in_guard(guard, source, offset) {
                    return Some(hit);
                }
            }
        }
    }
    None
}

fn find_func_span_in_guard(guard: &ast::Guard, source: &str, offset: usize) -> Option<(Span, String)> {
    match guard {
        ast::Guard::Test(e) => find_func_span_in_expr(e, source, offset),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                if let Some(hit) = find_func_span_in_expr(e, source, offset) { return Some(hit); }
            }
            None
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                if let Some(hit) = find_func_span_in_guard(g, source, offset) { return Some(hit); }
            }
            None
        }
    }
}

fn find_func_span_in_exprs(exprs: &[ast::Expr], source: &str, offset: usize) -> Option<(Span, String)> {
    for e in exprs {
        if let Some(hit) = find_func_span_in_expr(e, source, offset) { return Some(hit); }
    }
    None
}

fn find_func_span_in_expr(expr: &ast::Expr, source: &str, offset: usize) -> Option<(Span, String)> {
    if !in_span(offset, expr_span(expr)) { return None; }
    match expr {
        ast::Expr::Call(ast::CallTarget::Local(_, name_span), args, _) => {
            if in_span(offset, *name_span) {
                return Some((*name_span, span_text(source, *name_span).to_string()));
            }
            find_func_span_in_exprs(args, source, offset)
        }
        ast::Expr::Call(ast::CallTarget::Remote(mod_expr, fun_expr), args, _) => {
            if let ast::Expr::Atom(_, fs) = fun_expr.as_ref() {
                if in_span(offset, *fs) {
                    return Some((*fs, span_text(source, *fs).to_string()));
                }
            }
            if let Some(hit) = find_func_span_in_expr(mod_expr, source, offset) { return Some(hit); }
            if let Some(hit) = find_func_span_in_expr(fun_expr, source, offset) { return Some(hit); }
            find_func_span_in_exprs(args, source, offset)
        }
        ast::Expr::Call(_, args, _) => find_func_span_in_exprs(args, source, offset),
        ast::Expr::FunRef(_, full_span) => {
            // Walk back from the '/' to find the identifier.
            let text = span_text(source, *full_span);
            let slash = text.rfind('/')?;
            let tb = text.as_bytes();
            let mut start = slash;
            while start > 0 {
                let c = tb[start - 1];
                if c.is_ascii_alphanumeric() || c == b'_' || c == b'@' {
                    start -= 1;
                } else {
                    break;
                }
            }
            if start == slash { return None; }
            let name_span = Span { start: full_span.start + start, end: full_span.start + slash };
            if in_span(offset, name_span) {
                return Some((name_span, text[start..slash].to_string()));
            }
            None
        }
        ast::Expr::Tuple(elems, _) => find_func_span_in_exprs(elems, source, offset),
        ast::Expr::Cons(h, t, _) => {
            find_func_span_in_expr(h, source, offset)
                .or_else(|| find_func_span_in_expr(t, source, offset))
        }
        ast::Expr::BinOp(_, l, r, _) | ast::Expr::Send(l, r, _) => {
            find_func_span_in_expr(l, source, offset)
                .or_else(|| find_func_span_in_expr(r, source, offset))
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            find_func_span_in_expr(e, source, offset)
        }
        ast::Expr::Match(_, rhs, _) => find_func_span_in_expr(rhs, source, offset),
        ast::Expr::Case(scrutinee, clauses, _) => {
            if let Some(hit) = find_func_span_in_expr(scrutinee, source, offset) { return Some(hit); }
            for c in clauses {
                if let Some(g) = &c.guard {
                    if let Some(hit) = find_func_span_in_guard(g, source, offset) { return Some(hit); }
                }
                if let Some(hit) = find_func_span_in_exprs(&c.body, source, offset) { return Some(hit); }
            }
            None
        }
        ast::Expr::If(clauses, _) => {
            for c in clauses {
                if let Some(hit) = find_func_span_in_guard(&c.guard, source, offset) { return Some(hit); }
                if let Some(hit) = find_func_span_in_exprs(&c.body, source, offset) { return Some(hit); }
            }
            None
        }
        ast::Expr::Receive(clauses, after, _) => {
            for c in clauses {
                if let Some(g) = &c.guard {
                    if let Some(hit) = find_func_span_in_guard(g, source, offset) { return Some(hit); }
                }
                if let Some(hit) = find_func_span_in_exprs(&c.body, source, offset) { return Some(hit); }
            }
            if let Some(a) = after {
                if let Some(hit) = find_func_span_in_expr(&a.timeout, source, offset) { return Some(hit); }
                if let Some(hit) = find_func_span_in_exprs(&a.body, source, offset) { return Some(hit); }
            }
            None
        }
        ast::Expr::Block(body, _) => find_func_span_in_exprs(body, source, offset),
        ast::Expr::Binary(segs, _) => {
            for seg in segs {
                if let Some(hit) = find_func_span_in_expr(&seg.value, source, offset) { return Some(hit); }
                if let Some(size) = &seg.size {
                    if let Some(hit) = find_func_span_in_expr(size, source, offset) { return Some(hit); }
                }
            }
            None
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                if let Some(hit) = find_func_span_in_expr(&f.key, source, offset) { return Some(hit); }
                if let Some(hit) = find_func_span_in_expr(&f.value, source, offset) { return Some(hit); }
            }
            None
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            if let Some(hit) = find_func_span_in_expr(base, source, offset) { return Some(hit); }
            for f in fields {
                if let Some(hit) = find_func_span_in_expr(&f.key, source, offset) { return Some(hit); }
                if let Some(hit) = find_func_span_in_expr(&f.value, source, offset) { return Some(hit); }
            }
            None
        }
        ast::Expr::ListComp(body, quals, _) | ast::Expr::BinComp(body, quals, _) => {
            if let Some(hit) = find_func_span_in_expr(body, source, offset) { return Some(hit); }
            for q in quals {
                match q {
                    ast::LcQualifier::Generator(_, e, _)
                    | ast::LcQualifier::BinGenerator(_, e, _)
                    | ast::LcQualifier::Filter(e, _) => {
                        if let Some(hit) = find_func_span_in_expr(e, source, offset) { return Some(hit); }
                    }
                }
            }
            None
        }
        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            if let Some(hit) = find_func_span_in_exprs(body, source, offset) { return Some(hit); }
            for c in of_clauses {
                if let Some(g) = &c.guard {
                    if let Some(hit) = find_func_span_in_guard(g, source, offset) { return Some(hit); }
                }
                if let Some(hit) = find_func_span_in_exprs(&c.body, source, offset) { return Some(hit); }
            }
            for c in catch_clauses {
                if let Some(hit) = find_func_span_in_exprs(&c.body, source, offset) { return Some(hit); }
            }
            if let Some(b) = after_body {
                if let Some(hit) = find_func_span_in_exprs(b, source, offset) { return Some(hit); }
            }
            None
        }
        ast::Expr::Fun(_, clauses, _) => {
            for c in clauses {
                if let Some(hit) = find_func_span_in_exprs(&c.body, source, offset) { return Some(hit); }
            }
            None
        }
        ast::Expr::RecordCreate { fields, .. } | ast::Expr::RecordUpdate { fields, .. } => {
            if let ast::Expr::RecordUpdate { base, .. } = expr {
                if let Some(hit) = find_func_span_in_expr(base, source, offset) { return Some(hit); }
            }
            for f in fields {
                if let Some(hit) = find_func_span_in_expr(&f.value, source, offset) { return Some(hit); }
            }
            None
        }
        ast::Expr::RecordAccess { base, .. } => find_func_span_in_expr(base, source, offset),
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) | ast::MaybeExpr::CondMatch(_, e, _) => {
                        if let Some(hit) = find_func_span_in_expr(e, source, offset) { return Some(hit); }
                    }
                }
            }
            for c in else_clauses {
                if let Some(g) = &c.guard {
                    if let Some(hit) = find_func_span_in_guard(g, source, offset) { return Some(hit); }
                }
                if let Some(hit) = find_func_span_in_exprs(&c.body, source, offset) { return Some(hit); }
            }
            None
        }
        _ => None,
    }
}

/// Given the cursor `offset`, determine the function-rename target (defining
/// module + name + arity). The `module` / `source` are for the currently-open
/// file; if the cursor target is a remote reference, the defining module may
/// be in a different file.
pub(crate) fn function_rename_target_at(
    module: &ast::Module,
    source: &str,
    offset: usize,
) -> Option<FunctionRenameTarget> {
    let cur_mod = module_name_of(module, source);

    // Definition name / clause name.
    for func in &module.functions {
        if in_span(offset, func.name_span) {
            return Some(FunctionRenameTarget {
                owner_module: cur_mod,
                name: span_text(source, func.name_span).to_string(),
                arity: func.arity,
            });
        }
        for clause in &func.clauses {
            if in_span(offset, clause.name_span) {
                return Some(FunctionRenameTarget {
                    owner_module: cur_mod,
                    name: span_text(source, clause.name_span).to_string(),
                    arity: func.arity,
                });
            }
        }
    }
    // Export entry.
    for fa in &module.exports {
        if in_span(offset, fa.span) {
            let text = span_text(source, fa.span);
            let end = text.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '@').unwrap_or(text.len());
            return Some(FunctionRenameTarget {
                owner_module: cur_mod,
                name: text[..end].to_string(),
                arity: fa.arity,
            });
        }
    }
    // Spec/callback name.
    for spec in &module.specs {
        if in_span(offset, spec.name_span) {
            return Some(FunctionRenameTarget {
                owner_module: cur_mod,
                name: span_text(source, spec.name_span).to_string(),
                arity: spec.arity,
            });
        }
    }
    // Local / remote calls and fun refs inside clause bodies.
    for func in &module.functions {
        if !in_span(offset, func.span) { continue; }
        for clause in &func.clauses {
            if !in_span(offset, clause.span) { continue; }
            if let Some(t) = find_rename_target_in_exprs(&clause.body, source, offset, &cur_mod) {
                return Some(t);
            }
            if let Some(guard) = &clause.guard {
                if let Some(t) = find_rename_target_in_guard(guard, source, offset, &cur_mod) {
                    return Some(t);
                }
            }
        }
    }
    None
}

fn find_rename_target_in_guard(guard: &ast::Guard, source: &str, offset: usize, cur_mod: &str) -> Option<FunctionRenameTarget> {
    match guard {
        ast::Guard::Test(e) => find_rename_target_in_expr(e, source, offset, cur_mod),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                if let Some(t) = find_rename_target_in_expr(e, source, offset, cur_mod) { return Some(t); }
            }
            None
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                if let Some(t) = find_rename_target_in_guard(g, source, offset, cur_mod) { return Some(t); }
            }
            None
        }
    }
}

fn find_rename_target_in_exprs(exprs: &[ast::Expr], source: &str, offset: usize, cur_mod: &str) -> Option<FunctionRenameTarget> {
    for e in exprs {
        if let Some(t) = find_rename_target_in_expr(e, source, offset, cur_mod) { return Some(t); }
    }
    None
}

fn find_rename_target_in_expr(expr: &ast::Expr, source: &str, offset: usize, cur_mod: &str) -> Option<FunctionRenameTarget> {
    if !in_span(offset, expr_span(expr)) { return None; }
    match expr {
        ast::Expr::Call(ast::CallTarget::Local(_, name_span), args, _) => {
            if in_span(offset, *name_span) {
                return Some(FunctionRenameTarget {
                    owner_module: cur_mod.to_string(),
                    name: span_text(source, *name_span).to_string(),
                    arity: args.len() as u32,
                });
            }
            find_rename_target_in_exprs(args, source, offset, cur_mod)
        }
        ast::Expr::Call(ast::CallTarget::Remote(mod_expr, fun_expr), args, _) => {
            if let (ast::Expr::Atom(_, ms), ast::Expr::Atom(_, fs)) =
                (mod_expr.as_ref(), fun_expr.as_ref())
            {
                if in_span(offset, *fs) || in_span(offset, *ms) {
                    return Some(FunctionRenameTarget {
                        owner_module: span_text(source, *ms).to_string(),
                        name: span_text(source, *fs).to_string(),
                        arity: args.len() as u32,
                    });
                }
            }
            if let Some(t) = find_rename_target_in_expr(mod_expr, source, offset, cur_mod) { return Some(t); }
            if let Some(t) = find_rename_target_in_expr(fun_expr, source, offset, cur_mod) { return Some(t); }
            find_rename_target_in_exprs(args, source, offset, cur_mod)
        }
        ast::Expr::Call(_, args, _) => find_rename_target_in_exprs(args, source, offset, cur_mod),
        ast::Expr::FunRef(target, full_span) => {
            if !in_span(offset, *full_span) { return None; }
            let text = span_text(source, *full_span);
            let slash = text.rfind('/')?;
            let tb = text.as_bytes();
            let name_end = slash;
            let mut name_start = slash;
            while name_start > 0 {
                let c = tb[name_start - 1];
                if c.is_ascii_alphanumeric() || c == b'_' || c == b'@' {
                    name_start -= 1;
                } else {
                    break;
                }
            }
            if name_start == name_end { return None; }
            let name = text[name_start..name_end].to_string();
            match target {
                ast::FunRefTarget::Local(_, arity) => Some(FunctionRenameTarget {
                    owner_module: cur_mod.to_string(),
                    name,
                    arity: *arity,
                }),
                ast::FunRefTarget::Remote(_, _, arity) => {
                    // Extract module name: it appears before the ':' that
                    // precedes the name atom.
                    let before_name = &text[..name_start];
                    let colon = before_name.rfind(':')?;
                    let mb = before_name.as_bytes();
                    let mut mstart = colon;
                    while mstart > 0 {
                        let c = mb[mstart - 1];
                        if c.is_ascii_alphanumeric() || c == b'_' || c == b'@' {
                            mstart -= 1;
                        } else {
                            break;
                        }
                    }
                    let module = before_name[mstart..colon].to_string();
                    Some(FunctionRenameTarget { owner_module: module, name, arity: *arity })
                }
                ast::FunRefTarget::DynamicRemote(_, _, _) => None,
            }
        }
        ast::Expr::Tuple(elems, _) => find_rename_target_in_exprs(elems, source, offset, cur_mod),
        ast::Expr::Cons(h, t, _) => {
            find_rename_target_in_expr(h, source, offset, cur_mod)
                .or_else(|| find_rename_target_in_expr(t, source, offset, cur_mod))
        }
        ast::Expr::BinOp(_, l, r, _) | ast::Expr::Send(l, r, _) => {
            find_rename_target_in_expr(l, source, offset, cur_mod)
                .or_else(|| find_rename_target_in_expr(r, source, offset, cur_mod))
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            find_rename_target_in_expr(e, source, offset, cur_mod)
        }
        ast::Expr::Match(_, rhs, _) => find_rename_target_in_expr(rhs, source, offset, cur_mod),
        ast::Expr::Case(scrutinee, clauses, _) => {
            if let Some(t) = find_rename_target_in_expr(scrutinee, source, offset, cur_mod) { return Some(t); }
            for c in clauses {
                if let Some(g) = &c.guard {
                    if let Some(t) = find_rename_target_in_guard(g, source, offset, cur_mod) { return Some(t); }
                }
                if let Some(t) = find_rename_target_in_exprs(&c.body, source, offset, cur_mod) { return Some(t); }
            }
            None
        }
        ast::Expr::If(clauses, _) => {
            for c in clauses {
                if let Some(t) = find_rename_target_in_guard(&c.guard, source, offset, cur_mod) { return Some(t); }
                if let Some(t) = find_rename_target_in_exprs(&c.body, source, offset, cur_mod) { return Some(t); }
            }
            None
        }
        ast::Expr::Receive(clauses, after, _) => {
            for c in clauses {
                if let Some(g) = &c.guard {
                    if let Some(t) = find_rename_target_in_guard(g, source, offset, cur_mod) { return Some(t); }
                }
                if let Some(t) = find_rename_target_in_exprs(&c.body, source, offset, cur_mod) { return Some(t); }
            }
            if let Some(a) = after {
                if let Some(t) = find_rename_target_in_expr(&a.timeout, source, offset, cur_mod) { return Some(t); }
                if let Some(t) = find_rename_target_in_exprs(&a.body, source, offset, cur_mod) { return Some(t); }
            }
            None
        }
        ast::Expr::Block(body, _) => find_rename_target_in_exprs(body, source, offset, cur_mod),
        ast::Expr::Binary(segs, _) => {
            for seg in segs {
                if let Some(t) = find_rename_target_in_expr(&seg.value, source, offset, cur_mod) { return Some(t); }
                if let Some(size) = &seg.size {
                    if let Some(t) = find_rename_target_in_expr(size, source, offset, cur_mod) { return Some(t); }
                }
            }
            None
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                if let Some(t) = find_rename_target_in_expr(&f.key, source, offset, cur_mod) { return Some(t); }
                if let Some(t) = find_rename_target_in_expr(&f.value, source, offset, cur_mod) { return Some(t); }
            }
            None
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            if let Some(t) = find_rename_target_in_expr(base, source, offset, cur_mod) { return Some(t); }
            for f in fields {
                if let Some(t) = find_rename_target_in_expr(&f.key, source, offset, cur_mod) { return Some(t); }
                if let Some(t) = find_rename_target_in_expr(&f.value, source, offset, cur_mod) { return Some(t); }
            }
            None
        }
        ast::Expr::ListComp(body, quals, _) | ast::Expr::BinComp(body, quals, _) => {
            if let Some(t) = find_rename_target_in_expr(body, source, offset, cur_mod) { return Some(t); }
            for q in quals {
                match q {
                    ast::LcQualifier::Generator(_, e, _)
                    | ast::LcQualifier::BinGenerator(_, e, _)
                    | ast::LcQualifier::Filter(e, _) => {
                        if let Some(t) = find_rename_target_in_expr(e, source, offset, cur_mod) { return Some(t); }
                    }
                }
            }
            None
        }
        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            if let Some(t) = find_rename_target_in_exprs(body, source, offset, cur_mod) { return Some(t); }
            for c in of_clauses {
                if let Some(g) = &c.guard {
                    if let Some(t) = find_rename_target_in_guard(g, source, offset, cur_mod) { return Some(t); }
                }
                if let Some(t) = find_rename_target_in_exprs(&c.body, source, offset, cur_mod) { return Some(t); }
            }
            for c in catch_clauses {
                if let Some(t) = find_rename_target_in_exprs(&c.body, source, offset, cur_mod) { return Some(t); }
            }
            if let Some(b) = after_body {
                if let Some(t) = find_rename_target_in_exprs(b, source, offset, cur_mod) { return Some(t); }
            }
            None
        }
        ast::Expr::Fun(_, clauses, _) => {
            for c in clauses {
                if let Some(t) = find_rename_target_in_exprs(&c.body, source, offset, cur_mod) { return Some(t); }
            }
            None
        }
        ast::Expr::RecordCreate { fields, .. } | ast::Expr::RecordUpdate { fields, .. } => {
            if let ast::Expr::RecordUpdate { base, .. } = expr {
                if let Some(t) = find_rename_target_in_expr(base, source, offset, cur_mod) { return Some(t); }
            }
            for f in fields {
                if let Some(t) = find_rename_target_in_expr(&f.value, source, offset, cur_mod) { return Some(t); }
            }
            None
        }
        ast::Expr::RecordAccess { base, .. } => find_rename_target_in_expr(base, source, offset, cur_mod),
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) | ast::MaybeExpr::CondMatch(_, e, _) => {
                        if let Some(t) = find_rename_target_in_expr(e, source, offset, cur_mod) { return Some(t); }
                    }
                }
            }
            for c in else_clauses {
                if let Some(g) = &c.guard {
                    if let Some(t) = find_rename_target_in_guard(g, source, offset, cur_mod) { return Some(t); }
                }
                if let Some(t) = find_rename_target_in_exprs(&c.body, source, offset, cur_mod) { return Some(t); }
            }
            None
        }
        _ => None,
    }
}

/// Collect all name-spans within a module that should be renamed when
/// renaming a function defined in that module. This includes:
///  - each function-clause `name_span` matching name+arity
///  - the export list entries with matching name+arity
///  - spec/callback names with matching name+arity
///  - local call sites and local fun refs
pub(crate) fn collect_defining_module_spans(
    module: &ast::Module,
    source: &str,
    name: &str,
    arity: u32,
) -> Vec<Span> {
    let mut spans = Vec::new();
    // Function definitions: each clause's name_span (covers both single- and
    // multi-clause functions, and includes the principal `name_span`).
    for func in &module.functions {
        if func.arity != arity { continue; }
        let fname = span_text(source, func.name_span);
        if fname != name { continue; }
        for clause in &func.clauses {
            spans.push(clause.name_span);
        }
    }
    // Export entries.
    for fa in &module.exports {
        if fa.arity != arity { continue; }
        let text = span_text(source, fa.span);
        let end = text.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '@').unwrap_or(text.len());
        if &text[..end] == name {
            spans.push(Span { start: fa.span.start, end: fa.span.start + end });
        }
    }
    // Spec / callback names.
    for spec in &module.specs {
        if spec.arity != arity { continue; }
        let sname = span_text(source, spec.name_span);
        if sname == name {
            spans.push(spec.name_span);
        }
    }
    // Local call sites + local fun refs.
    spans.extend(find_local_call_name_spans(module, source, name, arity));
    spans
}

/// Like `find_local_call_spans`, but returns the span covering only the function
/// name identifier (suitable for rename edits), not the full call expression.
pub(crate) fn find_local_call_name_spans(module: &ast::Module, source: &str, name: &str, arity: u32) -> Vec<Span> {
    find_local_call_spans(module, source, name, arity)
        .into_iter()
        .filter_map(|s| name_subspan_before(source, s, name, b"(/"))
        .collect()
}

/// Like `find_remote_call_spans`, but returns name-only spans (see `find_local_call_name_spans`).
pub(crate) fn find_remote_call_name_spans(
    module: &ast::Module,
    source: &str,
    target_mod: &str,
    name: &str,
    arity: u32,
) -> Vec<Span> {
    find_remote_call_spans(module, source, target_mod, name, arity)
        .into_iter()
        .filter_map(|s| name_subspan_before(source, s, name, b"(/"))
        .collect()
}

/// Find the first occurrence of `name` within `outer`'s source text where
/// it is preceded by a non-identifier byte and followed by one of `follow`.
fn name_subspan_before(source: &str, outer: Span, name: &str, follow: &[u8]) -> Option<Span> {
    let text = span_text(source, outer);
    let nb = name.as_bytes();
    let tb = text.as_bytes();
    if nb.is_empty() || nb.len() > tb.len() { return None; }
    let mut i = 0;
    while i + nb.len() <= tb.len() {
        if &tb[i..i + nb.len()] == nb {
            let prev_ok = i == 0 || {
                let c = tb[i - 1];
                !(c.is_ascii_alphanumeric() || c == b'_' || c == b'@')
            };
            if prev_ok {
                // Look at the first non-whitespace char after the name.
                let mut j = i + nb.len();
                while j < tb.len() && (tb[j] == b' ' || tb[j] == b'\t') { j += 1; }
                if j < tb.len() && follow.contains(&tb[j]) {
                    return Some(Span {
                        start: outer.start + i,
                        end: outer.start + i + nb.len(),
                    });
                }
            }
        }
        i += 1;
    }
    None
}

/// Recursively collect call spans in a list of expressions.
/// If `target_mod` is None, searches for local calls; if Some, searches for remote calls.
fn collect_call_spans_in_exprs(
    exprs: &[ast::Expr], source: &str,
    target_mod: Option<&str>, name: &str, arity: u32,
    out: &mut Vec<Span>,
) {
    for expr in exprs {
        collect_call_spans_in_expr(expr, source, target_mod, name, arity, out);
    }
}

fn collect_call_spans_in_guard(
    guard: &ast::Guard, source: &str,
    target_mod: Option<&str>, name: &str, arity: u32,
    out: &mut Vec<Span>,
) {
    match guard {
        ast::Guard::Test(e) => collect_call_spans_in_expr(e, source, target_mod, name, arity, out),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                collect_call_spans_in_expr(e, source, target_mod, name, arity, out);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                collect_call_spans_in_guard(g, source, target_mod, name, arity, out);
            }
        }
    }
}

fn collect_call_spans_in_expr(
    expr: &ast::Expr, source: &str,
    target_mod: Option<&str>, name: &str, arity: u32,
    out: &mut Vec<Span>,
) {
    match expr {
        ast::Expr::Call(ast::CallTarget::Remote(mod_expr, fun_expr), args, span) => {
            if let Some(tm) = target_mod {
                if let (ast::Expr::Atom(_, mod_span), ast::Expr::Atom(_, fun_span)) =
                    (mod_expr.as_ref(), fun_expr.as_ref())
                {
                    let m = span_text(source, *mod_span);
                    let f = span_text(source, *fun_span);
                    if m == tm && f == name && args.len() as u32 == arity {
                        out.push(*span);
                    }
                }
            }
            collect_call_spans_in_expr(mod_expr, source, target_mod, name, arity, out);
            collect_call_spans_in_expr(fun_expr, source, target_mod, name, arity, out);
            collect_call_spans_in_exprs(args, source, target_mod, name, arity, out);
        }
        ast::Expr::Call(ast::CallTarget::Local(_, call_span), args, span) => {
            if target_mod.is_none() {
                let f = span_text(source, *call_span);
                if f == name && args.len() as u32 == arity {
                    out.push(*span);
                }
            }
            collect_call_spans_in_exprs(args, source, target_mod, name, arity, out);
        }
        ast::Expr::Call(_, args, _) => {
            collect_call_spans_in_exprs(args, source, target_mod, name, arity, out);
        }
        ast::Expr::FunRef(ast::FunRefTarget::Remote(_mod_id, _fun_id, ref_arity), span) => {
            if let Some(tm) = target_mod {
                let ref_text = span_text(source, *span);
                let expected = format!("{}:{}/{}", tm, name, arity);
                if ref_text.contains(&expected) && *ref_arity == arity {
                    out.push(*span);
                }
            }
        }
        ast::Expr::FunRef(ast::FunRefTarget::Local(_, ref_arity), span) => {
            if target_mod.is_none() {
                let ref_text = span_text(source, *span);
                let expected = format!("{}/{}", name, arity);
                if ref_text.contains(&expected) && *ref_arity == arity {
                    out.push(*span);
                }
            }
        }
        ast::Expr::FunRef(ast::FunRefTarget::DynamicRemote(m, f, a), _) => {
            collect_call_spans_in_expr(m, source, target_mod, name, arity, out);
            collect_call_spans_in_expr(f, source, target_mod, name, arity, out);
            collect_call_spans_in_expr(a, source, target_mod, name, arity, out);
        }
        // Recurse into sub-expressions
        ast::Expr::Tuple(elems, _) => collect_call_spans_in_exprs(elems, source, target_mod, name, arity, out),
        ast::Expr::Cons(h, t, _) => {
            collect_call_spans_in_expr(h, source, target_mod, name, arity, out);
            collect_call_spans_in_expr(t, source, target_mod, name, arity, out);
        }
        ast::Expr::BinOp(_, l, r, _) | ast::Expr::Send(l, r, _) => {
            collect_call_spans_in_expr(l, source, target_mod, name, arity, out);
            collect_call_spans_in_expr(r, source, target_mod, name, arity, out);
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            collect_call_spans_in_expr(e, source, target_mod, name, arity, out);
        }
        ast::Expr::Match(_, rhs, _) => collect_call_spans_in_expr(rhs, source, target_mod, name, arity, out),
        ast::Expr::Case(scrutinee, clauses, _) => {
            collect_call_spans_in_expr(scrutinee, source, target_mod, name, arity, out);
            for c in clauses {
                if let Some(g) = &c.guard { collect_call_spans_in_guard(g, source, target_mod, name, arity, out); }
                collect_call_spans_in_exprs(&c.body, source, target_mod, name, arity, out);
            }
        }
        ast::Expr::If(clauses, _) => {
            for c in clauses {
                collect_call_spans_in_guard(&c.guard, source, target_mod, name, arity, out);
                collect_call_spans_in_exprs(&c.body, source, target_mod, name, arity, out);
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for c in clauses {
                if let Some(g) = &c.guard { collect_call_spans_in_guard(g, source, target_mod, name, arity, out); }
                collect_call_spans_in_exprs(&c.body, source, target_mod, name, arity, out);
            }
            if let Some(a) = after {
                collect_call_spans_in_expr(&a.timeout, source, target_mod, name, arity, out);
                collect_call_spans_in_exprs(&a.body, source, target_mod, name, arity, out);
            }
        }
        ast::Expr::Block(body, _) => collect_call_spans_in_exprs(body, source, target_mod, name, arity, out),
        ast::Expr::Binary(segs, _) => {
            for seg in segs {
                collect_call_spans_in_expr(&seg.value, source, target_mod, name, arity, out);
                if let Some(size) = &seg.size { collect_call_spans_in_expr(size, source, target_mod, name, arity, out); }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                collect_call_spans_in_expr(&f.key, source, target_mod, name, arity, out);
                collect_call_spans_in_expr(&f.value, source, target_mod, name, arity, out);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            collect_call_spans_in_expr(base, source, target_mod, name, arity, out);
            for f in fields {
                collect_call_spans_in_expr(&f.key, source, target_mod, name, arity, out);
                collect_call_spans_in_expr(&f.value, source, target_mod, name, arity, out);
            }
        }
        ast::Expr::ListComp(body, quals, _) | ast::Expr::BinComp(body, quals, _) => {
            collect_call_spans_in_expr(body, source, target_mod, name, arity, out);
            for q in quals {
                match q {
                    ast::LcQualifier::Generator(_, e, _) | ast::LcQualifier::BinGenerator(_, e, _) => {
                        collect_call_spans_in_expr(e, source, target_mod, name, arity, out);
                    }
                    ast::LcQualifier::Filter(e, _) => {
                        collect_call_spans_in_expr(e, source, target_mod, name, arity, out);
                    }
                }
            }
        }
        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            collect_call_spans_in_exprs(body, source, target_mod, name, arity, out);
            for c in of_clauses {
                if let Some(g) = &c.guard { collect_call_spans_in_guard(g, source, target_mod, name, arity, out); }
                collect_call_spans_in_exprs(&c.body, source, target_mod, name, arity, out);
            }
            for c in catch_clauses {
                collect_call_spans_in_exprs(&c.body, source, target_mod, name, arity, out);
            }
            if let Some(b) = after_body { collect_call_spans_in_exprs(b, source, target_mod, name, arity, out); }
        }
        ast::Expr::Fun(_, clauses, _) => {
            for c in clauses {
                collect_call_spans_in_exprs(&c.body, source, target_mod, name, arity, out);
            }
        }
        ast::Expr::RecordCreate { fields, .. } | ast::Expr::RecordUpdate { fields, .. } => {
            for f in fields {
                collect_call_spans_in_expr(&f.value, source, target_mod, name, arity, out);
            }
            if let ast::Expr::RecordUpdate { base, .. } = expr {
                collect_call_spans_in_expr(base, source, target_mod, name, arity, out);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            collect_call_spans_in_expr(base, source, target_mod, name, arity, out);
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) | ast::MaybeExpr::CondMatch(_, e, _) => {
                        collect_call_spans_in_expr(e, source, target_mod, name, arity, out);
                    }
                }
            }
            for c in else_clauses {
                if let Some(g) = &c.guard { collect_call_spans_in_guard(g, source, target_mod, name, arity, out); }
                collect_call_spans_in_exprs(&c.body, source, target_mod, name, arity, out);
            }
        }
        // Leaf nodes
        ast::Expr::Integer(..) | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(..) | ast::Expr::Atom(..)
        | ast::Expr::String(..) | ast::Expr::Nil(..) | ast::Expr::Variable(..)
        | ast::Expr::RecordIndex { .. }
        | ast::Expr::MacroCall { .. } => {}
    }
}

// ── Record reference span helpers ────────────────────────────────────

/// Extract record name from a span's source text (e.g. `#person{...}` → `"person"`).
fn extract_record_name_from_span(source: &str, span: Span) -> Option<&str> {
    let text = span_text(source, span);
    let hash_pos = text.find('#')?;
    let after_hash = &text[hash_pos + 1..];
    let name_end = after_hash.find(['{', '.']).unwrap_or(after_hash.len());
    let name = after_hash[..name_end].trim();
    if name.is_empty() { None } else { Some(name) }
}

/// Find all record usage spans in a module matching `record_name`.
pub(crate) fn find_record_ref_spans(
    module: &ast::Module,
    source: &str,
    record_name: &str,
) -> Vec<Span> {
    let mut spans = Vec::new();
    for func in &module.functions {
        for clause in &func.clauses {
            for pat in &clause.patterns {
                collect_record_spans_in_pattern(pat, source, record_name, &mut spans);
            }
            if let Some(guard) = &clause.guard {
                collect_record_spans_in_guard(guard, source, record_name, &mut spans);
            }
            collect_record_spans_in_exprs(&clause.body, source, record_name, &mut spans);
        }
    }
    spans
}

fn collect_record_spans_in_exprs(
    exprs: &[ast::Expr], source: &str, record_name: &str, out: &mut Vec<Span>,
) {
    for e in exprs {
        collect_record_spans_in_expr(e, source, record_name, out);
    }
}

fn collect_record_spans_in_guard(
    guard: &ast::Guard, source: &str, record_name: &str, out: &mut Vec<Span>,
) {
    match guard {
        ast::Guard::Test(e) => collect_record_spans_in_expr(e, source, record_name, out),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                collect_record_spans_in_expr(e, source, record_name, out);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                collect_record_spans_in_guard(g, source, record_name, out);
            }
        }
    }
}

fn collect_record_spans_in_expr(
    expr: &ast::Expr, source: &str, record_name: &str, out: &mut Vec<Span>,
) {
    match expr {
        ast::Expr::RecordCreate { span, fields, .. } => {
            if extract_record_name_from_span(source, *span) == Some(record_name) {
                out.push(*span);
            }
            for f in fields {
                collect_record_spans_in_expr(&f.value, source, record_name, out);
            }
        }
        ast::Expr::RecordUpdate { span, base, fields, .. } => {
            if extract_record_name_from_span(source, *span) == Some(record_name) {
                out.push(*span);
            }
            collect_record_spans_in_expr(base, source, record_name, out);
            for f in fields {
                collect_record_spans_in_expr(&f.value, source, record_name, out);
            }
        }
        ast::Expr::RecordAccess { span, base, .. } => {
            if extract_record_name_from_span(source, *span) == Some(record_name) {
                out.push(*span);
            }
            collect_record_spans_in_expr(base, source, record_name, out);
        }
        ast::Expr::RecordIndex { span, .. } => {
            if extract_record_name_from_span(source, *span) == Some(record_name) {
                out.push(*span);
            }
        }
        // Recurse into all sub-expressions
        ast::Expr::Call(target, args, _) => {
            if let ast::CallTarget::Remote(m, f) = target {
                collect_record_spans_in_expr(m, source, record_name, out);
                collect_record_spans_in_expr(f, source, record_name, out);
            }
            collect_record_spans_in_exprs(args, source, record_name, out);
        }
        ast::Expr::Tuple(elems, _) | ast::Expr::Block(elems, _) => {
            collect_record_spans_in_exprs(elems, source, record_name, out);
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::BinOp(_, h, t, _) | ast::Expr::Send(h, t, _) => {
            collect_record_spans_in_expr(h, source, record_name, out);
            collect_record_spans_in_expr(t, source, record_name, out);
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            collect_record_spans_in_expr(e, source, record_name, out);
        }
        ast::Expr::Match(_, rhs, _) => {
            collect_record_spans_in_expr(rhs, source, record_name, out);
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            collect_record_spans_in_expr(scrutinee, source, record_name, out);
            for c in clauses {
                collect_record_spans_in_pattern(&c.pattern, source, record_name, out);
                if let Some(g) = &c.guard { collect_record_spans_in_guard(g, source, record_name, out); }
                collect_record_spans_in_exprs(&c.body, source, record_name, out);
            }
        }
        ast::Expr::If(clauses, _) => {
            for c in clauses {
                collect_record_spans_in_guard(&c.guard, source, record_name, out);
                collect_record_spans_in_exprs(&c.body, source, record_name, out);
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for c in clauses {
                collect_record_spans_in_pattern(&c.pattern, source, record_name, out);
                if let Some(g) = &c.guard { collect_record_spans_in_guard(g, source, record_name, out); }
                collect_record_spans_in_exprs(&c.body, source, record_name, out);
            }
            if let Some(a) = after {
                collect_record_spans_in_expr(&a.timeout, source, record_name, out);
                collect_record_spans_in_exprs(&a.body, source, record_name, out);
            }
        }
        ast::Expr::Binary(segs, _) => {
            for seg in segs {
                collect_record_spans_in_expr(&seg.value, source, record_name, out);
                if let Some(size) = &seg.size { collect_record_spans_in_expr(size, source, record_name, out); }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                collect_record_spans_in_expr(&f.key, source, record_name, out);
                collect_record_spans_in_expr(&f.value, source, record_name, out);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            collect_record_spans_in_expr(base, source, record_name, out);
            for f in fields {
                collect_record_spans_in_expr(&f.key, source, record_name, out);
                collect_record_spans_in_expr(&f.value, source, record_name, out);
            }
        }
        ast::Expr::ListComp(body, quals, _) | ast::Expr::BinComp(body, quals, _) => {
            collect_record_spans_in_expr(body, source, record_name, out);
            for q in quals {
                match q {
                    ast::LcQualifier::Generator(pat, e, _) | ast::LcQualifier::BinGenerator(pat, e, _) => {
                        collect_record_spans_in_pattern(pat, source, record_name, out);
                        collect_record_spans_in_expr(e, source, record_name, out);
                    }
                    ast::LcQualifier::Filter(e, _) => {
                        collect_record_spans_in_expr(e, source, record_name, out);
                    }
                }
            }
        }
        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            collect_record_spans_in_exprs(body, source, record_name, out);
            for c in of_clauses {
                collect_record_spans_in_pattern(&c.pattern, source, record_name, out);
                if let Some(g) = &c.guard { collect_record_spans_in_guard(g, source, record_name, out); }
                collect_record_spans_in_exprs(&c.body, source, record_name, out);
            }
            for c in catch_clauses {
                collect_record_spans_in_pattern(&c.pattern, source, record_name, out);
                collect_record_spans_in_exprs(&c.body, source, record_name, out);
            }
            if let Some(b) = after_body { collect_record_spans_in_exprs(b, source, record_name, out); }
        }
        ast::Expr::Fun(_, clauses, _) => {
            for c in clauses {
                for pat in &c.patterns {
                    collect_record_spans_in_pattern(pat, source, record_name, out);
                }
                if let Some(g) = &c.guard { collect_record_spans_in_guard(g, source, record_name, out); }
                collect_record_spans_in_exprs(&c.body, source, record_name, out);
            }
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) | ast::MaybeExpr::CondMatch(_, e, _) => {
                        collect_record_spans_in_expr(e, source, record_name, out);
                    }
                }
            }
            for c in else_clauses {
                collect_record_spans_in_pattern(&c.pattern, source, record_name, out);
                if let Some(g) = &c.guard { collect_record_spans_in_guard(g, source, record_name, out); }
                collect_record_spans_in_exprs(&c.body, source, record_name, out);
            }
        }
        // Leaf nodes
        ast::Expr::Integer(..) | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(..) | ast::Expr::Atom(..)
        | ast::Expr::String(..) | ast::Expr::Nil(..) | ast::Expr::Variable(..)
        | ast::Expr::FunRef(..)
        | ast::Expr::MacroCall { .. } => {}
    }
}

fn collect_record_spans_in_pattern(
    pat: &ast::Pattern, source: &str, record_name: &str, out: &mut Vec<Span>,
) {
    match pat {
        ast::Pattern::Record { span, fields, .. } => {
            if extract_record_name_from_span(source, *span) == Some(record_name) {
                out.push(*span);
            }
            for f in fields {
                collect_record_spans_in_pattern(&f.pattern, source, record_name, out);
            }
        }
        ast::Pattern::Tuple(pats, _) => {
            for p in pats { collect_record_spans_in_pattern(p, source, record_name, out); }
        }
        ast::Pattern::Cons(h, t, _) => {
            collect_record_spans_in_pattern(h, source, record_name, out);
            collect_record_spans_in_pattern(t, source, record_name, out);
        }
        ast::Pattern::Match(l, r, _) => {
            collect_record_spans_in_pattern(l, source, record_name, out);
            collect_record_spans_in_pattern(r, source, record_name, out);
        }
        ast::Pattern::Map(fields, _) => {
            for f in fields { collect_record_spans_in_pattern(&f.value, source, record_name, out); }
        }
        ast::Pattern::Binary(segs, _) => {
            for seg in segs { collect_record_spans_in_pattern(&seg.value, source, record_name, out); }
        }
        ast::Pattern::Concat(_, r, _) => {
            collect_record_spans_in_pattern(r, source, record_name, out);
        }
        _ => {}
    }
}

// ── Variable span helpers ────────────────────────────────────────────

/// Find all variable occurrences (bindings and references) in a function clause.
pub(crate) fn find_variable_spans_in_clause(clause: &ast::FunctionClause, source: &str, var_name: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    // Collect from patterns
    for pat in &clause.patterns {
        collect_var_spans_in_pattern(pat, source, var_name, &mut spans);
    }
    // Collect from guard
    if let Some(guard) = &clause.guard {
        collect_var_spans_in_guard(guard, source, var_name, &mut spans);
    }
    // Collect from body
    collect_var_spans_in_exprs(&clause.body, source, var_name, &mut spans);
    spans
}

fn collect_var_spans_in_pattern(pat: &ast::Pattern, source: &str, var_name: &str, out: &mut Vec<Span>) {
    match pat {
        ast::Pattern::Variable(_, span) => {
            if span_text(source, *span) == var_name { out.push(*span); }
        }
        ast::Pattern::Tuple(pats, _) => {
            for p in pats { collect_var_spans_in_pattern(p, source, var_name, out); }
        }
        ast::Pattern::Cons(h, t, _) => {
            collect_var_spans_in_pattern(h, source, var_name, out);
            collect_var_spans_in_pattern(t, source, var_name, out);
        }
        ast::Pattern::Match(l, r, _) => {
            collect_var_spans_in_pattern(l, source, var_name, out);
            collect_var_spans_in_pattern(r, source, var_name, out);
        }
        ast::Pattern::Record { fields, .. } => {
            for f in fields { collect_var_spans_in_pattern(&f.pattern, source, var_name, out); }
        }
        ast::Pattern::Map(fields, _) => {
            for f in fields { collect_var_spans_in_pattern(&f.value, source, var_name, out); }
        }
        ast::Pattern::Binary(segs, _) => {
            for seg in segs { collect_var_spans_in_pattern(&seg.value, source, var_name, out); }
        }
        ast::Pattern::Concat(l, r, _) => {
            collect_var_spans_in_pattern(l, source, var_name, out);
            collect_var_spans_in_pattern(r, source, var_name, out);
        }
        _ => {} // Literals, wildcards
    }
}

fn collect_var_spans_in_guard(guard: &ast::Guard, source: &str, var_name: &str, out: &mut Vec<Span>) {
    match guard {
        ast::Guard::Test(e) => collect_var_spans_in_expr(e, source, var_name, out),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs { collect_var_spans_in_expr(e, source, var_name, out); }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards { collect_var_spans_in_guard(g, source, var_name, out); }
        }
    }
}

fn collect_var_spans_in_exprs(exprs: &[ast::Expr], source: &str, var_name: &str, out: &mut Vec<Span>) {
    for e in exprs { collect_var_spans_in_expr(e, source, var_name, out); }
}

fn collect_var_spans_in_expr(expr: &ast::Expr, source: &str, var_name: &str, out: &mut Vec<Span>) {
    match expr {
        ast::Expr::Variable(_, span) => {
            if span_text(source, *span) == var_name { out.push(*span); }
        }
        ast::Expr::Call(target, args, _) => {
            match target {
                ast::CallTarget::Local(_, _) => {}
                ast::CallTarget::Remote(m, f) => {
                    collect_var_spans_in_expr(m, source, var_name, out);
                    collect_var_spans_in_expr(f, source, var_name, out);
                }
                ast::CallTarget::Variable(_, span) => {
                    if span_text(source, *span) == var_name { out.push(*span); }
                }
                ast::CallTarget::Expr(callee) => {
                    collect_var_spans_in_expr(callee, source, var_name, out);
                }
            }
            collect_var_spans_in_exprs(args, source, var_name, out);
        }
        ast::Expr::Tuple(elems, _) => collect_var_spans_in_exprs(elems, source, var_name, out),
        ast::Expr::Cons(h, t, _) | ast::Expr::BinOp(_, h, t, _) | ast::Expr::Send(h, t, _) => {
            collect_var_spans_in_expr(h, source, var_name, out);
            collect_var_spans_in_expr(t, source, var_name, out);
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            collect_var_spans_in_expr(e, source, var_name, out);
        }
        ast::Expr::Match(pat, rhs, _) => {
            collect_var_spans_in_pattern_from_match(pat, source, var_name, out);
            collect_var_spans_in_expr(rhs, source, var_name, out);
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            collect_var_spans_in_expr(scrutinee, source, var_name, out);
            for c in clauses {
                collect_var_spans_in_pattern(&c.pattern, source, var_name, out);
                if let Some(g) = &c.guard { collect_var_spans_in_guard(g, source, var_name, out); }
                collect_var_spans_in_exprs(&c.body, source, var_name, out);
            }
        }
        ast::Expr::If(clauses, _) => {
            for c in clauses {
                collect_var_spans_in_guard(&c.guard, source, var_name, out);
                collect_var_spans_in_exprs(&c.body, source, var_name, out);
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for c in clauses {
                collect_var_spans_in_pattern(&c.pattern, source, var_name, out);
                if let Some(g) = &c.guard { collect_var_spans_in_guard(g, source, var_name, out); }
                collect_var_spans_in_exprs(&c.body, source, var_name, out);
            }
            if let Some(a) = after {
                collect_var_spans_in_expr(&a.timeout, source, var_name, out);
                collect_var_spans_in_exprs(&a.body, source, var_name, out);
            }
        }
        ast::Expr::Block(body, _) => collect_var_spans_in_exprs(body, source, var_name, out),
        ast::Expr::Binary(segs, _) => {
            for seg in segs {
                collect_var_spans_in_expr(&seg.value, source, var_name, out);
                if let Some(size) = &seg.size { collect_var_spans_in_expr(size, source, var_name, out); }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                collect_var_spans_in_expr(&f.key, source, var_name, out);
                collect_var_spans_in_expr(&f.value, source, var_name, out);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            collect_var_spans_in_expr(base, source, var_name, out);
            for f in fields {
                collect_var_spans_in_expr(&f.key, source, var_name, out);
                collect_var_spans_in_expr(&f.value, source, var_name, out);
            }
        }
        ast::Expr::ListComp(body, quals, _) | ast::Expr::BinComp(body, quals, _) => {
            collect_var_spans_in_expr(body, source, var_name, out);
            for q in quals {
                match q {
                    ast::LcQualifier::Generator(pat, e, _) | ast::LcQualifier::BinGenerator(pat, e, _) => {
                        collect_var_spans_in_pattern(pat, source, var_name, out);
                        collect_var_spans_in_expr(e, source, var_name, out);
                    }
                    ast::LcQualifier::Filter(e, _) => {
                        collect_var_spans_in_expr(e, source, var_name, out);
                    }
                }
            }
        }
        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            collect_var_spans_in_exprs(body, source, var_name, out);
            for c in of_clauses {
                collect_var_spans_in_pattern(&c.pattern, source, var_name, out);
                if let Some(g) = &c.guard { collect_var_spans_in_guard(g, source, var_name, out); }
                collect_var_spans_in_exprs(&c.body, source, var_name, out);
            }
            for c in catch_clauses {
                collect_var_spans_in_pattern(&c.pattern, source, var_name, out);
                if let Some(st) = &c.stacktrace { collect_var_spans_in_pattern(st, source, var_name, out); }
                if let Some(g) = &c.guard { collect_var_spans_in_guard(g, source, var_name, out); }
                collect_var_spans_in_exprs(&c.body, source, var_name, out);
            }
            if let Some(b) = after_body { collect_var_spans_in_exprs(b, source, var_name, out); }
        }
        ast::Expr::Fun(_, clauses, _) => {
            for c in clauses {
                for pat in &c.patterns { collect_var_spans_in_pattern(pat, source, var_name, out); }
                if let Some(g) = &c.guard { collect_var_spans_in_guard(g, source, var_name, out); }
                collect_var_spans_in_exprs(&c.body, source, var_name, out);
            }
        }
        ast::Expr::RecordCreate { fields, .. } | ast::Expr::RecordUpdate { fields, .. } => {
            for f in fields { collect_var_spans_in_expr(&f.value, source, var_name, out); }
            if let ast::Expr::RecordUpdate { base, .. } = expr {
                collect_var_spans_in_expr(base, source, var_name, out);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            collect_var_spans_in_expr(base, source, var_name, out);
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => collect_var_spans_in_expr(e, source, var_name, out),
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        collect_var_spans_in_pattern(pat, source, var_name, out);
                        collect_var_spans_in_expr(e, source, var_name, out);
                    }
                }
            }
            for c in else_clauses {
                collect_var_spans_in_pattern(&c.pattern, source, var_name, out);
                if let Some(g) = &c.guard { collect_var_spans_in_guard(g, source, var_name, out); }
                collect_var_spans_in_exprs(&c.body, source, var_name, out);
            }
        }
        _ => {}
    }
}

fn collect_var_spans_in_pattern_from_match(pat: &ast::Pattern, source: &str, var_name: &str, out: &mut Vec<Span>) {
    collect_var_spans_in_pattern(pat, source, var_name, out);
}

// ── Variable highlight helpers (read vs write) ──────────────────────

/// Find all variable occurrences in a clause, classified as Write (binding
/// sites in patterns) or Read (use sites in expressions / guards).
pub(crate) fn find_variable_highlights_in_clause(
    clause: &ast::FunctionClause,
    source: &str,
    var_name: &str,
) -> Vec<(Span, DocumentHighlightKind)> {
    let mut writes: Vec<Span> = Vec::new();
    let mut reads: Vec<Span> = Vec::new();
    for pat in &clause.patterns {
        collect_var_spans_in_pattern(pat, source, var_name, &mut writes);
    }
    if let Some(guard) = &clause.guard {
        collect_var_spans_in_guard(guard, source, var_name, &mut reads);
    }
    collect_var_highlights_in_exprs(&clause.body, source, var_name, &mut writes, &mut reads);
    let mut result: Vec<(Span, DocumentHighlightKind)> = writes
        .into_iter()
        .map(|s| (s, DocumentHighlightKind::WRITE))
        .collect();
    result.extend(reads.into_iter().map(|s| (s, DocumentHighlightKind::READ)));
    result
}

fn collect_var_highlights_in_exprs(
    exprs: &[ast::Expr],
    source: &str,
    var_name: &str,
    writes: &mut Vec<Span>,
    reads: &mut Vec<Span>,
) {
    for e in exprs {
        collect_var_highlights_in_expr(e, source, var_name, writes, reads);
    }
}

fn collect_var_highlights_in_expr(
    expr: &ast::Expr,
    source: &str,
    var_name: &str,
    writes: &mut Vec<Span>,
    reads: &mut Vec<Span>,
) {
    match expr {
        ast::Expr::Variable(_, span) => {
            if span_text(source, *span) == var_name { reads.push(*span); }
        }
        ast::Expr::Call(target, args, _) => {
            match target {
                ast::CallTarget::Local(_, _) => {}
                ast::CallTarget::Remote(m, f) => {
                    collect_var_highlights_in_expr(m, source, var_name, writes, reads);
                    collect_var_highlights_in_expr(f, source, var_name, writes, reads);
                }
                ast::CallTarget::Variable(_, span) => {
                    if span_text(source, *span) == var_name { reads.push(*span); }
                }
                ast::CallTarget::Expr(callee) => {
                    collect_var_highlights_in_expr(callee, source, var_name, writes, reads);
                }
            }
            collect_var_highlights_in_exprs(args, source, var_name, writes, reads);
        }
        ast::Expr::Tuple(elems, _) => {
            collect_var_highlights_in_exprs(elems, source, var_name, writes, reads);
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::BinOp(_, h, t, _) | ast::Expr::Send(h, t, _) => {
            collect_var_highlights_in_expr(h, source, var_name, writes, reads);
            collect_var_highlights_in_expr(t, source, var_name, writes, reads);
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            collect_var_highlights_in_expr(e, source, var_name, writes, reads);
        }
        ast::Expr::Match(pat, rhs, _) => {
            collect_var_spans_in_pattern(pat, source, var_name, writes);
            collect_var_highlights_in_expr(rhs, source, var_name, writes, reads);
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            collect_var_highlights_in_expr(scrutinee, source, var_name, writes, reads);
            for c in clauses {
                collect_var_spans_in_pattern(&c.pattern, source, var_name, writes);
                if let Some(g) = &c.guard { collect_var_spans_in_guard(g, source, var_name, reads); }
                collect_var_highlights_in_exprs(&c.body, source, var_name, writes, reads);
            }
        }
        ast::Expr::If(clauses, _) => {
            for c in clauses {
                collect_var_spans_in_guard(&c.guard, source, var_name, reads);
                collect_var_highlights_in_exprs(&c.body, source, var_name, writes, reads);
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for c in clauses {
                collect_var_spans_in_pattern(&c.pattern, source, var_name, writes);
                if let Some(g) = &c.guard { collect_var_spans_in_guard(g, source, var_name, reads); }
                collect_var_highlights_in_exprs(&c.body, source, var_name, writes, reads);
            }
            if let Some(a) = after {
                collect_var_highlights_in_expr(&a.timeout, source, var_name, writes, reads);
                collect_var_highlights_in_exprs(&a.body, source, var_name, writes, reads);
            }
        }
        ast::Expr::Block(body, _) => {
            collect_var_highlights_in_exprs(body, source, var_name, writes, reads);
        }
        ast::Expr::Binary(segs, _) => {
            for seg in segs {
                collect_var_highlights_in_expr(&seg.value, source, var_name, writes, reads);
                if let Some(size) = &seg.size {
                    collect_var_highlights_in_expr(size, source, var_name, writes, reads);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                collect_var_highlights_in_expr(&f.key, source, var_name, writes, reads);
                collect_var_highlights_in_expr(&f.value, source, var_name, writes, reads);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            collect_var_highlights_in_expr(base, source, var_name, writes, reads);
            for f in fields {
                collect_var_highlights_in_expr(&f.key, source, var_name, writes, reads);
                collect_var_highlights_in_expr(&f.value, source, var_name, writes, reads);
            }
        }
        ast::Expr::ListComp(body, quals, _) | ast::Expr::BinComp(body, quals, _) => {
            collect_var_highlights_in_expr(body, source, var_name, writes, reads);
            for q in quals {
                match q {
                    ast::LcQualifier::Generator(pat, e, _) | ast::LcQualifier::BinGenerator(pat, e, _) => {
                        collect_var_spans_in_pattern(pat, source, var_name, writes);
                        collect_var_highlights_in_expr(e, source, var_name, writes, reads);
                    }
                    ast::LcQualifier::Filter(e, _) => {
                        collect_var_highlights_in_expr(e, source, var_name, writes, reads);
                    }
                }
            }
        }
        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            collect_var_highlights_in_exprs(body, source, var_name, writes, reads);
            for c in of_clauses {
                collect_var_spans_in_pattern(&c.pattern, source, var_name, writes);
                if let Some(g) = &c.guard { collect_var_spans_in_guard(g, source, var_name, reads); }
                collect_var_highlights_in_exprs(&c.body, source, var_name, writes, reads);
            }
            for c in catch_clauses {
                collect_var_spans_in_pattern(&c.pattern, source, var_name, writes);
                if let Some(st) = &c.stacktrace { collect_var_spans_in_pattern(st, source, var_name, writes); }
                if let Some(g) = &c.guard { collect_var_spans_in_guard(g, source, var_name, reads); }
                collect_var_highlights_in_exprs(&c.body, source, var_name, writes, reads);
            }
            if let Some(b) = after_body {
                collect_var_highlights_in_exprs(b, source, var_name, writes, reads);
            }
        }
        ast::Expr::Fun(_, clauses, _) => {
            for c in clauses {
                for pat in &c.patterns { collect_var_spans_in_pattern(pat, source, var_name, writes); }
                if let Some(g) = &c.guard { collect_var_spans_in_guard(g, source, var_name, reads); }
                collect_var_highlights_in_exprs(&c.body, source, var_name, writes, reads);
            }
        }
        ast::Expr::RecordCreate { fields, .. } | ast::Expr::RecordUpdate { fields, .. } => {
            for f in fields {
                collect_var_highlights_in_expr(&f.value, source, var_name, writes, reads);
            }
            if let ast::Expr::RecordUpdate { base, .. } = expr {
                collect_var_highlights_in_expr(base, source, var_name, writes, reads);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            collect_var_highlights_in_expr(base, source, var_name, writes, reads);
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => {
                        collect_var_highlights_in_expr(e, source, var_name, writes, reads);
                    }
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        collect_var_spans_in_pattern(pat, source, var_name, writes);
                        collect_var_highlights_in_expr(e, source, var_name, writes, reads);
                    }
                }
            }
            for c in else_clauses {
                collect_var_spans_in_pattern(&c.pattern, source, var_name, writes);
                if let Some(g) = &c.guard { collect_var_spans_in_guard(g, source, var_name, reads); }
                collect_var_highlights_in_exprs(&c.body, source, var_name, writes, reads);
            }
        }
        _ => {}
    }
}

// ── Inlay hint helpers ──────────────────────────────────────────────

/// Collect inlay hints (parameter names) from call expressions in a list of expressions.
pub(crate) fn collect_inlay_hints_from_exprs(
    exprs: &[ast::Expr],
    module: &ast::Module,
    source: &str,
    sm: &SourceMap,
    project_modules: &Arc<Mutex<Vec<ParsedModuleCache>>>,
    enclosing_span: Span,
    out: &mut Vec<InlayHint>,
) {
    for expr in exprs {
        collect_inlay_hints_from_expr(expr, module, source, sm, project_modules, enclosing_span, out);
    }
}

fn collect_inlay_hints_from_expr(
    expr: &ast::Expr,
    module: &ast::Module,
    source: &str,
    sm: &SourceMap,
    project_modules: &Arc<Mutex<Vec<ParsedModuleCache>>>,
    enclosing_span: Span,
    out: &mut Vec<InlayHint>,
) {
    match expr {
        ast::Expr::Call(target, args, _) => {
            // Try to resolve parameter names for this call
            let param_names = match target {
                ast::CallTarget::Local(_, name_span) => {
                    let func_name = span_text(source, *name_span);
                    let arity = args.len() as u32;
                    // Find in current module, fall back to BIF table (auto-imported from erlang)
                    get_local_param_names(module, source, func_name, arity)
                        .or_else(|| {
                            bif_table::lookup_bif_param_names("erlang", func_name, arity)
                                .map(|names| names.iter().map(|s| s.to_string()).collect())
                        })
                }
                ast::CallTarget::Remote(mod_expr, fun_expr) => {
                    if let (ast::Expr::Atom(_, mod_span), ast::Expr::Atom(_, fun_span)) =
                        (mod_expr.as_ref(), fun_expr.as_ref())
                    {
                        let mod_name = span_text(source, *mod_span);
                        let fun_name = span_text(source, *fun_span);
                        let arity = args.len() as u32;
                        // Find in project modules, fall back to BIF table
                        get_remote_param_names(project_modules, mod_name, fun_name, arity)
                            .or_else(|| {
                                bif_table::lookup_bif_param_names(mod_name, fun_name, arity)
                                    .map(|names| names.iter().map(|s| s.to_string()).collect())
                            })
                    } else {
                        None
                    }
                }
                ast::CallTarget::Variable(_, _) => None,
                ast::CallTarget::Expr(_) => None,
            };

            // Emit inlay hints for each argument if we found parameter names
            if let Some(names) = param_names {
                for (i, arg) in args.iter().enumerate() {
                    if let Some(name) = names.get(i) {
                        // Skip if the argument is already a simple variable with the same name
                        if let ast::Expr::Variable(_, var_span) = arg {
                            let var_name = span_text(source, *var_span);
                            if var_name.to_lowercase() == name.to_lowercase() {
                                continue;
                            }
                        }
                        // Skip generic names like single-letter vars
                        if name.len() <= 1 || name == "_" {
                            continue;
                        }
                        let arg_span = expr_span(arg);
                        // Skip hints for macro-expanded arguments whose spans
                        // point back to the macro definition site.
                        if arg_span.start < enclosing_span.start || arg_span.end > enclosing_span.end {
                            continue;
                        }
                        let position = span_to_lsp_range(arg_span, sm).start;
                        out.push(InlayHint {
                            position,
                            label: InlayHintLabel::String(format!("{}:", name)),
                            kind: Some(InlayHintKind::PARAMETER),
                            text_edits: None,
                            tooltip: None,
                            padding_left: None,
                            padding_right: Some(true),
                            data: None,
                        });
                    }
                }
            }

            // Recurse into arguments
            for arg in args {
                collect_inlay_hints_from_expr(arg, module, source, sm, project_modules, enclosing_span, out);
            }
            // Recurse into module/function exprs for remote calls
            if let ast::CallTarget::Remote(m, f) = target {
                collect_inlay_hints_from_expr(m, module, source, sm, project_modules, enclosing_span, out);
                collect_inlay_hints_from_expr(f, module, source, sm, project_modules, enclosing_span, out);
            }
        }
        // Recurse into all sub-expressions
        ast::Expr::Tuple(elems, _) => {
            for e in elems { collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out); }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::BinOp(_, h, t, _) | ast::Expr::Send(h, t, _) => {
            collect_inlay_hints_from_expr(h, module, source, sm, project_modules, enclosing_span, out);
            collect_inlay_hints_from_expr(t, module, source, sm, project_modules, enclosing_span, out);
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out);
        }
        ast::Expr::Match(_, rhs, _) => {
            collect_inlay_hints_from_expr(rhs, module, source, sm, project_modules, enclosing_span, out);
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            collect_inlay_hints_from_expr(scrutinee, module, source, sm, project_modules, enclosing_span, out);
            for c in clauses {
                for e in &c.body { collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out); }
            }
        }
        ast::Expr::If(clauses, _) => {
            for c in clauses {
                for e in &c.body { collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out); }
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for c in clauses {
                for e in &c.body { collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out); }
            }
            if let Some(a) = after {
                collect_inlay_hints_from_expr(&a.timeout, module, source, sm, project_modules, enclosing_span, out);
                for e in &a.body { collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out); }
            }
        }
        ast::Expr::Block(body, _) => {
            for e in body { collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out); }
        }
        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            for e in body { collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out); }
            for c in of_clauses { for e in &c.body { collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out); } }
            for c in catch_clauses { for e in &c.body { collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out); } }
            if let Some(b) = after_body { for e in b { collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out); } }
        }
        ast::Expr::Fun(_, clauses, _) => {
            for c in clauses { for e in &c.body { collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out); } }
        }
        ast::Expr::RecordCreate { fields, .. } | ast::Expr::RecordUpdate { fields, .. } => {
            for f in fields { collect_inlay_hints_from_expr(&f.value, module, source, sm, project_modules, enclosing_span, out); }
            if let ast::Expr::RecordUpdate { base, .. } = expr {
                collect_inlay_hints_from_expr(base, module, source, sm, project_modules, enclosing_span, out);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            collect_inlay_hints_from_expr(base, module, source, sm, project_modules, enclosing_span, out);
        }
        ast::Expr::ListComp(body, quals, _) | ast::Expr::BinComp(body, quals, _) => {
            collect_inlay_hints_from_expr(body, module, source, sm, project_modules, enclosing_span, out);
            for q in quals {
                match q {
                    ast::LcQualifier::Generator(_, e, _) | ast::LcQualifier::BinGenerator(_, e, _) => {
                        collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out);
                    }
                    ast::LcQualifier::Filter(e, _) => {
                        collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out);
                    }
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                collect_inlay_hints_from_expr(&f.key, module, source, sm, project_modules, enclosing_span, out);
                collect_inlay_hints_from_expr(&f.value, module, source, sm, project_modules, enclosing_span, out);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            collect_inlay_hints_from_expr(base, module, source, sm, project_modules, enclosing_span, out);
            for f in fields {
                collect_inlay_hints_from_expr(&f.key, module, source, sm, project_modules, enclosing_span, out);
                collect_inlay_hints_from_expr(&f.value, module, source, sm, project_modules, enclosing_span, out);
            }
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) | ast::MaybeExpr::CondMatch(_, e, _) => {
                        collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out);
                    }
                }
            }
            for c in else_clauses { for e in &c.body { collect_inlay_hints_from_expr(e, module, source, sm, project_modules, enclosing_span, out); } }
        }
        ast::Expr::Binary(segs, _) => {
            for seg in segs {
                collect_inlay_hints_from_expr(&seg.value, module, source, sm, project_modules, enclosing_span, out);
                if let Some(size) = &seg.size { collect_inlay_hints_from_expr(size, module, source, sm, project_modules, enclosing_span, out); }
            }
        }
        // Leaf nodes
        _ => {}
    }
}

/// Get parameter names from a local function's first clause.
pub(crate) fn get_local_param_names(module: &ast::Module, source: &str, func_name: &str, arity: u32) -> Option<Vec<String>> {
    for func in &module.functions {
        let fname = span_text(source, func.name_span);
        if fname == func_name && func.arity == arity
            && !func.clauses.is_empty() {
                return Some(best_param_names(&func.clauses, source));
            }
    }
    None
}

/// Get parameter names from a remote function via project cache.
pub(crate) fn get_remote_param_names(
    project_modules: &Arc<Mutex<Vec<ParsedModuleCache>>>,
    mod_name: &str,
    fun_name: &str,
    arity: u32,
) -> Option<Vec<String>> {
    let modules = project_modules.lock().unwrap();
    for cached in modules.iter() {
        if cached.module_name == mod_name {
            for func in &cached.module.functions {
                let fname = span_text(&cached.source, func.name_span);
                if fname == fun_name && func.arity == arity
                    && !func.clauses.is_empty() {
                        return Some(best_param_names(&func.clauses, &cached.source));
                    }
            }
        }
    }
    None
}

/// Extract a readable parameter name from a pattern.
/// For variables, returns the variable name.
/// For complex patterns, returns a short description.
fn extract_param_name(source: &str, pat: &ast::Pattern) -> String {
    match pat {
        ast::Pattern::Variable(_, span) => span_text(source, *span).to_string(),
        ast::Pattern::Atom(_, span) => span_text(source, *span).to_string(),
        ast::Pattern::Tuple(_, span) => span_text(source, *span).to_string(),
        ast::Pattern::Record { span, .. } => span_text(source, *span).to_string(),
        ast::Pattern::Match(_, inner, _) => extract_param_name(source, inner),
        ast::Pattern::Wildcard(_) => "_".to_string(),
        _ => span_text(source, pattern_span(pat)).to_string(),
    }
}

/// Score a parameter name for relevance (higher = better).
/// 0: wildcard (`_`) or empty
/// 1: non-variable pattern (atom literal, tuple, list, cons, etc.)
/// 2: underscore-prefixed variable (`_Pred`)
/// 3: bare variable (`Pred`, `F`, `Acc`)
///
/// Length filtering (single-char names) is handled at display time,
/// not here — otherwise `F` (score 3) would lose to `_F` (score 2).
fn score_param_name(name: &str, pat: &ast::Pattern) -> u8 {
    if name == "_" || name.is_empty() {
        return 0;
    }
    match pat {
        ast::Pattern::Variable(_, _) => {
            if name.starts_with('_') { 2 } else { 3 }
        }
        _ => 1,
    }
}

/// Pick the best parameter name per position across all clauses.
///
/// Post-processing:
/// - If only non-variable patterns were found (score <= 1), the name is
///   cleared so the downstream display filter suppresses the hint.
/// - Underscore-prefixed names have their leading `_` stripped (e.g.
///   `_Pred` becomes `Pred`) so the hint reads naturally.
fn best_param_names(clauses: &[ast::FunctionClause], source: &str) -> Vec<String> {
    let arity = clauses.first().map_or(0, |c| c.patterns.len());
    let mut best_names: Vec<String> = Vec::with_capacity(arity);
    let mut best_scores: Vec<u8> = Vec::with_capacity(arity);

    for _ in 0..arity {
        best_names.push(String::new());
        best_scores.push(0);
    }

    for clause in clauses {
        for (i, pat) in clause.patterns.iter().enumerate() {
            if i >= arity {
                break;
            }
            let name = extract_param_name(source, pat);
            let score = score_param_name(&name, pat);
            if score > best_scores[i] {
                best_scores[i] = score;
                best_names[i] = name;
            }
        }
    }

    // Post-process: suppress pattern-only names and strip underscore prefixes.
    for i in 0..arity {
        if best_scores[i] <= 1 {
            // Only non-variable patterns — not useful as a hint.
            best_names[i].clear();
        } else if best_scores[i] == 2 && best_names[i].starts_with('_') {
            // Strip leading underscore for display (_Pred -> Pred).
            best_names[i] = best_names[i][1..].to_string();
        }
    }

    best_names
}
