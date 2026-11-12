use nera_syntax::ast;

use super::types::CursorTarget;
use super::util::{span_text, in_span, extract_funref_local_name, extract_funref_remote_names};

/// Walk the AST to find what is under the cursor at the given byte offset.
pub(crate) fn find_cursor_target(module: &ast::Module, offset: usize, source: &str) -> Option<CursorTarget> {
    for func in &module.functions {
        if !in_span(offset, func.span) {
            continue;
        }
        for clause in &func.clauses {
            if !in_span(offset, clause.span) {
                continue;
            }
            // Check patterns
            for pat in &clause.patterns {
                if let Some(t) = find_cursor_in_pattern(pat, offset, source) {
                    return Some(t);
                }
            }
            // Check guard
            if let Some(guard) = &clause.guard {
                if let Some(t) = find_cursor_in_guard(guard, offset, source) {
                    return Some(t);
                }
            }
            // Check body
            for expr in &clause.body {
                if let Some(t) = find_cursor_in_expr(expr, offset, source) {
                    return Some(t);
                }
            }
        }
    }
    None
}

pub(crate) fn find_cursor_in_pattern(pat: &ast::Pattern, offset: usize, source: &str) -> Option<CursorTarget> {
    match pat {
        ast::Pattern::Variable(_, span) if in_span(offset, *span) => {
            Some(CursorTarget::Variable { name: span_text(source, *span).to_string() })
        }
        ast::Pattern::Tuple(pats, _) => {
            for p in pats { if let Some(t) = find_cursor_in_pattern(p, offset, source) { return Some(t); } }
            None
        }
        ast::Pattern::Cons(h, t, _) => {
            find_cursor_in_pattern(h, offset, source)
                .or_else(|| find_cursor_in_pattern(t, offset, source))
        }
        ast::Pattern::Match(p1, p2, _) => {
            find_cursor_in_pattern(p1, offset, source)
                .or_else(|| find_cursor_in_pattern(p2, offset, source))
        }
        ast::Pattern::Binary(segments, _) => {
            for seg in segments {
                if let Some(t) = find_cursor_in_pattern(&seg.value, offset, source) { return Some(t); }
                if let Some(size) = &seg.size {
                    if let Some(t) = find_cursor_in_expr(size, offset, source) { return Some(t); }
                }
            }
            None
        }
        ast::Pattern::Map(fields, _) => {
            for field in fields {
                if let Some(t) = find_cursor_in_expr(&field.key, offset, source) { return Some(t); }
                if let Some(t) = find_cursor_in_pattern(&field.value, offset, source) { return Some(t); }
            }
            None
        }
        ast::Pattern::Record { record_name: _, fields, span } if in_span(offset, *span) => {
            // Check if cursor is on a field pattern
            for field in fields {
                if let Some(t) = find_cursor_in_pattern(&field.pattern, offset, source) {
                    return Some(t);
                }
            }
            // Cursor is on the record name part — extract it.
            // Record pattern syntax: #name{...}, the name starts after '#'
            let span_str = span_text(source, *span);
            if let Some(hash_pos) = span_str.find('#') {
                let after_hash = &span_str[hash_pos + 1..];
                let name_end = after_hash.find('{').unwrap_or(after_hash.len());
                let name = after_hash[..name_end].trim();
                if !name.is_empty() {
                    return Some(CursorTarget::RecordRef { name: name.to_string() });
                }
            }
            None
        }
        ast::Pattern::Concat(lhs, rhs, _) => {
            find_cursor_in_pattern(lhs, offset, source)
                .or_else(|| find_cursor_in_pattern(rhs, offset, source))
        }
        _ => None,
    }
}

pub(crate) fn find_cursor_in_guard(guard: &ast::Guard, offset: usize, source: &str) -> Option<CursorTarget> {
    match guard {
        ast::Guard::Test(expr) => find_cursor_in_expr(expr, offset, source),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs { if let Some(t) = find_cursor_in_expr(e, offset, source) { return Some(t); } }
            None
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards { if let Some(t) = find_cursor_in_guard(g, offset, source) { return Some(t); } }
            None
        }
    }
}

pub(crate) fn find_cursor_in_expr(expr: &ast::Expr, offset: usize, source: &str) -> Option<CursorTarget> {
    if !in_span(offset, expr.span()) {
        return None;
    }

    match expr {
        ast::Expr::Variable(_, span) => {
            Some(CursorTarget::Variable { name: span_text(source, *span).to_string() })
        }

        ast::Expr::Call(target, args, _) => {
            // Check call target first
            match target {
                ast::CallTarget::Local(_, name_span) if in_span(offset, *name_span) => {
                    return Some(CursorTarget::LocalCall {
                        name: span_text(source, *name_span).to_string(),
                        arity: args.len() as u32,
                    });
                }
                ast::CallTarget::Remote(mod_expr, fun_expr) => {
                    if let (ast::Expr::Atom(_, mod_span), ast::Expr::Atom(_, fun_span)) =
                        (mod_expr.as_ref(), fun_expr.as_ref())
                    {
                        if in_span(offset, *fun_span) || in_span(offset, *mod_span) {
                            return Some(CursorTarget::RemoteCall {
                                module: span_text(source, *mod_span).to_string(),
                                function: span_text(source, *fun_span).to_string(),
                                arity: args.len() as u32,
                            });
                        }
                    }
                    // Fall through to check args if cursor isn't on mod:fun
                    if let Some(t) = find_cursor_in_expr(mod_expr, offset, source) { return Some(t); }
                    if let Some(t) = find_cursor_in_expr(fun_expr, offset, source) { return Some(t); }
                }
                ast::CallTarget::Variable(_, span) if in_span(offset, *span) => {
                    return Some(CursorTarget::Variable { name: span_text(source, *span).to_string() });
                }
                _ => {}
            }
            // Check args
            for arg in args {
                if let Some(t) = find_cursor_in_expr(arg, offset, source) { return Some(t); }
            }
            None
        }

        ast::Expr::FunRef(target, span) if in_span(offset, *span) => {
            match target {
                ast::FunRefTarget::Local(_, arity) => {
                    // Extract name from span text: "fun name/arity"
                    let text = span_text(source, *span);
                    let name = extract_funref_local_name(text)?;
                    Some(CursorTarget::LocalFunRef { name, arity: *arity })
                }
                ast::FunRefTarget::Remote(_, _, arity) => {
                    let text = span_text(source, *span);
                    let (module, function) = extract_funref_remote_names(text)?;
                    Some(CursorTarget::RemoteFunRef { module, function, arity: *arity })
                }
                ast::FunRefTarget::DynamicRemote(m, f, a) => {
                    // Try sub-expressions for cursor hits
                    if let Some(t) = find_cursor_in_expr(m, offset, source) { return Some(t); }
                    if let Some(t) = find_cursor_in_expr(f, offset, source) { return Some(t); }
                    if let Some(t) = find_cursor_in_expr(a, offset, source) { return Some(t); }
                    None
                }
            }
        }

        ast::Expr::RecordCreate { record_name: _, span, .. }
        | ast::Expr::RecordUpdate { record_name: _, span, .. }
        | ast::Expr::RecordAccess { record_name: _, span, .. }
        | ast::Expr::RecordIndex { record_name: _, span, .. } => {
            // Try to recurse into sub-expressions first
            match expr {
                ast::Expr::RecordCreate { fields, .. } => {
                    for field in fields {
                        if let Some(t) = find_cursor_in_expr(&field.value, offset, source) { return Some(t); }
                    }
                }
                ast::Expr::RecordUpdate { base, fields, .. } => {
                    if let Some(t) = find_cursor_in_expr(base, offset, source) { return Some(t); }
                    for field in fields {
                        if let Some(t) = find_cursor_in_expr(&field.value, offset, source) { return Some(t); }
                    }
                }
                ast::Expr::RecordAccess { base, .. } => {
                    if let Some(t) = find_cursor_in_expr(base, offset, source) { return Some(t); }
                }
                _ => {}
            }
            // Cursor is on the record name — extract it from source
            let span_str = span_text(source, *span);
            if let Some(hash_pos) = span_str.find('#') {
                let after_hash = &span_str[hash_pos + 1..];
                let name_end = after_hash.find(['{', '.']).unwrap_or(after_hash.len());
                let name = after_hash[..name_end].trim();
                if !name.is_empty() {
                    return Some(CursorTarget::RecordRef { name: name.to_string() });
                }
            }
            None
        }

        // Container expressions — recurse
        ast::Expr::Match(pat, e, _) => {
            find_cursor_in_pattern(pat, offset, source)
                .or_else(|| find_cursor_in_expr(e, offset, source))
        }
        ast::Expr::Tuple(exprs, _) | ast::Expr::Block(exprs, _) => {
            for e in exprs { if let Some(t) = find_cursor_in_expr(e, offset, source) { return Some(t); } }
            None
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) => {
            find_cursor_in_expr(h, offset, source)
                .or_else(|| find_cursor_in_expr(t, offset, source))
        }
        ast::Expr::BinOp(_, lhs, rhs, _) => {
            find_cursor_in_expr(lhs, offset, source)
                .or_else(|| find_cursor_in_expr(rhs, offset, source))
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            find_cursor_in_expr(e, offset, source)
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            if let Some(t) = find_cursor_in_expr(scrutinee, offset, source) { return Some(t); }
            for clause in clauses { if let Some(t) = find_cursor_in_case_clause(clause, offset, source) { return Some(t); } }
            None
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                if in_span(offset, clause.span) {
                    if let Some(t) = find_cursor_in_guard(&clause.guard, offset, source) { return Some(t); }
                    for e in &clause.body { if let Some(t) = find_cursor_in_expr(e, offset, source) { return Some(t); } }
                }
            }
            None
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses { if let Some(t) = find_cursor_in_case_clause(clause, offset, source) { return Some(t); } }
            if let Some(after_clause) = after {
                if let Some(t) = find_cursor_in_expr(&after_clause.timeout, offset, source) { return Some(t); }
                for e in &after_clause.body { if let Some(t) = find_cursor_in_expr(e, offset, source) { return Some(t); } }
            }
            None
        }
        ast::Expr::Fun(_, clauses, _) => {
            for clause in clauses {
                if in_span(offset, clause.span) {
                    for pat in &clause.patterns { if let Some(t) = find_cursor_in_pattern(pat, offset, source) { return Some(t); } }
                    if let Some(guard) = &clause.guard { if let Some(t) = find_cursor_in_guard(guard, offset, source) { return Some(t); } }
                    for e in &clause.body { if let Some(t) = find_cursor_in_expr(e, offset, source) { return Some(t); } }
                }
            }
            None
        }
        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            for e in body { if let Some(t) = find_cursor_in_expr(e, offset, source) { return Some(t); } }
            for clause in of_clauses { if let Some(t) = find_cursor_in_case_clause(clause, offset, source) { return Some(t); } }
            for clause in catch_clauses {
                if in_span(offset, clause.span) {
                    if let Some(t) = find_cursor_in_pattern(&clause.pattern, offset, source) { return Some(t); }
                    if let Some(st) = &clause.stacktrace { if let Some(t) = find_cursor_in_pattern(st, offset, source) { return Some(t); } }
                    if let Some(guard) = &clause.guard { if let Some(t) = find_cursor_in_guard(guard, offset, source) { return Some(t); } }
                    for e in &clause.body { if let Some(t) = find_cursor_in_expr(e, offset, source) { return Some(t); } }
                }
            }
            if let Some(ab) = after_body { for e in ab { if let Some(t) = find_cursor_in_expr(e, offset, source) { return Some(t); } } }
            None
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                if let Some(t) = find_cursor_in_expr(&seg.value, offset, source) { return Some(t); }
                if let Some(size) = &seg.size { if let Some(t) = find_cursor_in_expr(size, offset, source) { return Some(t); } }
            }
            None
        }
        ast::Expr::Map(fields, _) => {
            for field in fields {
                if let Some(t) = find_cursor_in_expr(&field.key, offset, source) { return Some(t); }
                if let Some(t) = find_cursor_in_expr(&field.value, offset, source) { return Some(t); }
            }
            None
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            if let Some(t) = find_cursor_in_expr(base, offset, source) { return Some(t); }
            for field in fields {
                if let Some(t) = find_cursor_in_expr(&field.key, offset, source) { return Some(t); }
                if let Some(t) = find_cursor_in_expr(&field.value, offset, source) { return Some(t); }
            }
            None
        }
        ast::Expr::ListComp(body, quals, _) | ast::Expr::BinComp(body, quals, _) => {
            if let Some(t) = find_cursor_in_expr(body, offset, source) { return Some(t); }
            for qual in quals {
                match qual {
                    ast::LcQualifier::Generator(pat, e, _) | ast::LcQualifier::BinGenerator(pat, e, _) => {
                        if let Some(t) = find_cursor_in_pattern(pat, offset, source) { return Some(t); }
                        if let Some(t) = find_cursor_in_expr(e, offset, source) { return Some(t); }
                    }
                    ast::LcQualifier::Filter(e, _) => {
                        if let Some(t) = find_cursor_in_expr(e, offset, source) { return Some(t); }
                    }
                }
            }
            None
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => { if let Some(t) = find_cursor_in_expr(e, offset, source) { return Some(t); } }
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        if let Some(t) = find_cursor_in_pattern(pat, offset, source) { return Some(t); }
                        if let Some(t) = find_cursor_in_expr(e, offset, source) { return Some(t); }
                    }
                }
            }
            for clause in else_clauses { if let Some(t) = find_cursor_in_case_clause(clause, offset, source) { return Some(t); } }
            None
        }

        // Atoms, integers, floats, strings, nil — no target
        _ => None,
    }
}

pub(crate) fn find_cursor_in_case_clause(clause: &ast::CaseClause, offset: usize, source: &str) -> Option<CursorTarget> {
    if !in_span(offset, clause.span) { return None; }
    if let Some(t) = find_cursor_in_pattern(&clause.pattern, offset, source) { return Some(t); }
    if let Some(guard) = &clause.guard { if let Some(t) = find_cursor_in_guard(guard, offset, source) { return Some(t); } }
    for e in &clause.body { if let Some(t) = find_cursor_in_expr(e, offset, source) { return Some(t); } }
    None
}
