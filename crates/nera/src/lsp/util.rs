use nera_syntax::ast;
use nera_syntax::span::Span;

/// Extract the text at a span from the source.
pub(crate) fn span_text(source: &str, span: Span) -> &str {
    &source[span.start..span.end.min(source.len())]
}

/// Extract the span from a pattern node.
pub(crate) fn pattern_span(pat: &ast::Pattern) -> Span {
    match pat {
        ast::Pattern::Wildcard(s) => *s,
        ast::Pattern::Variable(_, s) => *s,
        ast::Pattern::Integer(_, s) | ast::Pattern::BigInteger(_, s) => *s,
        ast::Pattern::Float(_, s) => *s,
        ast::Pattern::Atom(_, s) => *s,
        ast::Pattern::String(_, s) => *s,
        ast::Pattern::Tuple(_, s) => *s,
        ast::Pattern::Cons(_, _, s) => *s,
        ast::Pattern::Nil(s) => *s,
        ast::Pattern::Match(_, _, s) => *s,
        ast::Pattern::Binary(_, s) => *s,
        ast::Pattern::Map(_, s) => *s,
        ast::Pattern::Record { span, .. } => *span,
        ast::Pattern::Concat(_, _, s) => *s,
        ast::Pattern::MacroCall { span, .. } => *span,
    }
}

/// Check if a byte offset falls within a span.
pub(crate) fn in_span(offset: usize, span: Span) -> bool {
    offset >= span.start && offset < span.end
}

/// Extract the span from an expression node.
pub(crate) fn expr_span(expr: &ast::Expr) -> Span {
    match expr {
        ast::Expr::Integer(_, s) | ast::Expr::BigInteger(_, s) => *s,
        ast::Expr::Float(_, s) => *s,
        ast::Expr::Atom(_, s) => *s,
        ast::Expr::String(_, s) => *s,
        ast::Expr::Variable(_, s) => *s,
        ast::Expr::Tuple(_, s) => *s,
        ast::Expr::Nil(s) => *s,
        ast::Expr::Cons(_, _, s) => *s,
        ast::Expr::Binary(_, s) => *s,
        ast::Expr::UnaryOp(_, _, s) => *s,
        ast::Expr::BinOp(_, _, _, s) => *s,
        ast::Expr::Match(_, _, s) => *s,
        ast::Expr::Call(_, _, s) => *s,
        ast::Expr::Send(_, _, s) => *s,
        ast::Expr::Case(_, _, s) => *s,
        ast::Expr::If(_, s) => *s,
        ast::Expr::Receive(_, _, s) => *s,
        ast::Expr::Fun(_, _, s) => *s,
        ast::Expr::FunRef(_, s) => *s,
        ast::Expr::Block(_, s) => *s,
        ast::Expr::Throw(_, s) => *s,
        ast::Expr::Catch(_, s) => *s,
        ast::Expr::Try { span, .. } => *span,
        ast::Expr::Map(_, s) => *s,
        ast::Expr::MapUpdate(_, _, s) => *s,
        ast::Expr::ListComp(_, _, s) => *s,
        ast::Expr::BinComp(_, _, s) => *s,
        ast::Expr::RecordCreate { span, .. } => *span,
        ast::Expr::RecordUpdate { span, .. } => *span,
        ast::Expr::RecordAccess { span, .. } => *span,
        ast::Expr::RecordIndex { span, .. } => *span,
        ast::Expr::Maybe { span, .. } => *span,
        ast::Expr::MacroCall { span, .. } => *span,
    }
}

/// Check if the cursor is on an `-include("...")` or `-include_lib("...")` directive.
///
/// Returns `Some(path)` with the include path string if found.
pub(crate) fn find_include_at_offset(source: &str, offset: usize) -> Option<String> {
    if offset >= source.len() {
        return None;
    }
    let line_start = source[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = source[offset..].find('\n').map(|i| offset + i).unwrap_or(source.len());
    let line = source[line_start..line_end].trim();

    // Match -include("path") or -include_lib("path")
    for prefix in &["-include(\"", "-include_lib(\""] {
        if let Some(start) = line.find(prefix) {
            let path_start = start + prefix.len();
            let rest = &line[path_start..];
            if let Some(end) = rest.find('"') {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

/// Check if the cursor is on a function name/arity inside an `-export([...])` attribute.
///
/// Returns `Some((function_name, arity))` if the offset falls within an export
/// list entry like `start/0` or `handle_call/3`.
pub(crate) fn find_export_entry_at_offset(source: &str, offset: usize) -> Option<(String, u32)> {
    if offset >= source.len() {
        return None;
    }

    // Quick check: is this line (or nearby) part of an -export attribute?
    // Look backwards from offset to find `-export` within a reasonable range.
    let search_start = offset.saturating_sub(500);
    let before_text = &source[search_start..offset];

    // Find the last `-export` or `-export_type` before the cursor
    let export_pos = before_text.rfind("-export(").or_else(|| before_text.rfind("-export ("));
    export_pos?;

    // Verify we haven't passed the closing `]).` — i.e., we're still inside the export list
    let after_export = &source[search_start + export_pos.unwrap()..offset];
    if after_export.contains("]).") {
        return None;
    }

    // Now extract the fun/arity at the cursor position.
    // Expand from the offset to find the full `name/arity` token.
    // The cursor could be on the name, the `/`, or the arity digits.
    let bytes = source.as_bytes();

    // Find the start of the full name/arity token (scan backwards over
    // alphanumeric, `_`, `/`, and digits).
    let mut start = offset;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_' || bytes[start - 1] == b'/') {
        start -= 1;
    }

    // Find the end of the token (scan forward)
    let mut end = offset;
    while end < source.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'/') {
        end += 1;
    }

    let token = &source[start..end];
    // Parse "name/arity"
    if let Some(slash_pos) = token.find('/') {
        let name = &token[..slash_pos];
        if let Ok(arity) = token[slash_pos + 1..].parse::<u32>() {
            if !name.is_empty() && name.chars().next().unwrap().is_ascii_lowercase() {
                return Some((name.to_string(), arity));
            }
        }
    }

    None
}

/// Detect a `?MACRO` token at the given byte offset in the original source.
///
/// Macros are expanded by the preprocessor and don't appear in the AST,
/// so we do a text-based scan of the original source to find them.
pub(crate) fn find_macro_at_offset(source: &str, offset: usize) -> Option<String> {
    if offset >= source.len() {
        return None;
    }
    let bytes = source.as_bytes();

    // Find the `?` that starts this macro reference.
    // Scan backwards from offset to find it (the cursor could be on `?` or anywhere in the name).
    let mut q_pos = offset;
    // First, if we're in the middle of an identifier, scan back to find `?`
    while q_pos > 0 && bytes[q_pos - 1].is_ascii_alphanumeric() || (q_pos > 0 && bytes[q_pos - 1] == b'_') {
        q_pos -= 1;
    }
    if q_pos > 0 && bytes[q_pos - 1] == b'?' {
        q_pos -= 1;
    }

    // Verify we found a `?`
    if bytes.get(q_pos).copied() != Some(b'?') {
        // Maybe offset is exactly on `?`
        if bytes.get(offset).copied() == Some(b'?') {
            q_pos = offset;
        } else {
            return None;
        }
    }

    // Collect the macro name after `?`
    let name_start = q_pos + 1;
    if name_start >= source.len() {
        return None;
    }
    // First char must be alphabetic or underscore
    if !bytes[name_start].is_ascii_alphabetic() && bytes[name_start] != b'_' {
        return None;
    }
    let mut name_end = name_start + 1;
    while name_end < source.len() && (bytes[name_end].is_ascii_alphanumeric() || bytes[name_end] == b'_') {
        name_end += 1;
    }

    // Check the cursor is within the `?NAME` range
    if offset < q_pos || offset >= name_end {
        return None;
    }

    let name = &source[name_start..name_end];
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// Extract the function name from `fun name/arity` text.
pub(crate) fn extract_funref_local_name(text: &str) -> Option<String> {
    // "fun name/2" → "name"
    let text = text.trim();
    let rest = text.strip_prefix("fun")?;
    let rest = rest.trim_start();
    let slash = rest.find('/')?;
    Some(rest[..slash].trim().to_string())
}

/// Extract module and function from `fun mod:fun/arity` text.
pub(crate) fn extract_funref_remote_names(text: &str) -> Option<(String, String)> {
    // "fun mod:fun/2" → ("mod", "fun")
    let text = text.trim();
    let rest = text.strip_prefix("fun")?;
    let rest = rest.trim_start();
    let colon = rest.find(':')?;
    let module = rest[..colon].trim().to_string();
    let after_colon = &rest[colon + 1..];
    let slash = after_colon.find('/')?;
    let function = after_colon[..slash].trim().to_string();
    Some((module, function))
}

/// Collect all variable bindings from a pattern, returning (name, span) pairs.
pub(crate) fn collect_pattern_bindings(pat: &ast::Pattern, source: &str, bindings: &mut Vec<(String, Span)>) {
    match pat {
        ast::Pattern::Variable(_, span) => {
            bindings.push((span_text(source, *span).to_string(), *span));
        }
        ast::Pattern::Tuple(pats, _) => {
            for p in pats { collect_pattern_bindings(p, source, bindings); }
        }
        ast::Pattern::Cons(h, t, _) | ast::Pattern::Match(h, t, _) | ast::Pattern::Concat(h, t, _) => {
            collect_pattern_bindings(h, source, bindings);
            collect_pattern_bindings(t, source, bindings);
        }
        ast::Pattern::Binary(segments, _) => {
            for seg in segments { collect_pattern_bindings(&seg.value, source, bindings); }
        }
        ast::Pattern::Map(fields, _) => {
            for field in fields { collect_pattern_bindings(&field.value, source, bindings); }
        }
        ast::Pattern::Record { fields, .. } => {
            for field in fields { collect_pattern_bindings(&field.pattern, source, bindings); }
        }
        _ => {}
    }
}

/// Collect variable bindings from patterns in body expressions (match, case, etc.).
pub(crate) fn collect_body_bindings(body: &[ast::Expr], source: &str, bindings: &mut Vec<(String, Span)>) {
    for expr in body {
        collect_expr_bindings(expr, source, bindings);
    }
}

pub(crate) fn collect_expr_bindings(expr: &ast::Expr, source: &str, bindings: &mut Vec<(String, Span)>) {
    match expr {
        ast::Expr::Match(pat, e, _) => {
            collect_pattern_bindings(pat, source, bindings);
            collect_expr_bindings(e, source, bindings);
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            collect_expr_bindings(scrutinee, source, bindings);
            for clause in clauses {
                collect_pattern_bindings(&clause.pattern, source, bindings);
                for e in &clause.body { collect_expr_bindings(e, source, bindings); }
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                collect_pattern_bindings(&clause.pattern, source, bindings);
                for e in &clause.body { collect_expr_bindings(e, source, bindings); }
            }
            if let Some(after_clause) = after {
                for e in &after_clause.body { collect_expr_bindings(e, source, bindings); }
            }
        }
        ast::Expr::Fun(_, clauses, _) => {
            for clause in clauses {
                for pat in &clause.patterns { collect_pattern_bindings(pat, source, bindings); }
                for e in &clause.body { collect_expr_bindings(e, source, bindings); }
            }
        }
        ast::Expr::Block(exprs, _) => {
            for e in exprs { collect_expr_bindings(e, source, bindings); }
        }
        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            for e in body { collect_expr_bindings(e, source, bindings); }
            for clause in of_clauses {
                collect_pattern_bindings(&clause.pattern, source, bindings);
                for e in &clause.body { collect_expr_bindings(e, source, bindings); }
            }
            for clause in catch_clauses {
                collect_pattern_bindings(&clause.pattern, source, bindings);
                if let Some(st) = &clause.stacktrace { collect_pattern_bindings(st, source, bindings); }
                for e in &clause.body { collect_expr_bindings(e, source, bindings); }
            }
            if let Some(ab) = after_body { for e in ab { collect_expr_bindings(e, source, bindings); } }
        }
        ast::Expr::ListComp(_, quals, _) | ast::Expr::BinComp(_, quals, _) => {
            for qual in quals {
                match qual {
                    ast::LcQualifier::Generator(pat, _, _) | ast::LcQualifier::BinGenerator(pat, _, _) => {
                        collect_pattern_bindings(pat, source, bindings);
                    }
                    _ => {}
                }
            }
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => collect_expr_bindings(e, source, bindings),
                    ast::MaybeExpr::CondMatch(pat, _, _) => {
                        collect_pattern_bindings(pat, source, bindings);
                    }
                }
            }
            for clause in else_clauses {
                collect_pattern_bindings(&clause.pattern, source, bindings);
                for e in &clause.body { collect_expr_bindings(e, source, bindings); }
            }
        }
        _ => {}
    }
}

/// Extract the variable name from an unused variable diagnostic message.
///
/// Parses `"variable 'X' is unused"` → `Some("X")`.
pub(crate) fn extract_variable_name(message: &str) -> Option<&str> {
    let start = message.find("variable '")? + "variable '".len();
    let end = message[start..].find('\'')?;
    Some(&message[start..start + end])
}

/// Find the line number (0-based) immediately after the `-module(Name).`
/// attribute in `raw_text`, suitable for inserting a new attribute on a
/// fresh line. Returns `0` if no module line is found.
pub(crate) fn find_line_after_module_attr(raw_text: &str) -> u32 {
    for (idx, line) in raw_text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("-module(") || trimmed.starts_with("-module (") {
            return (idx as u32) + 1;
        }
    }
    0
}

/// Extract `(name, arity)` from a diagnostic message that contains a
/// `'name/arity'` token. Works for:
///
/// - `"function 'foo/2' is unused"` (unused_function)
/// - `"undefined callback function 'init/1' (behaviour 'gen_server')"`
///   (missing_behaviour_callback)
pub(crate) fn extract_function_name_arity(message: &str) -> Option<(&str, u32)> {
    let start = message.find('\'')? + 1;
    let rest = &message[start..];
    let end = rest.find('\'')?;
    let token = &rest[..end];
    let slash = token.find('/')?;
    let name = &token[..slash];
    let arity: u32 = token[slash + 1..].parse().ok()?;
    if name.is_empty() { return None; }
    Some((name, arity))
}

/// Extract (module_name, file_stem) from a module-name-mismatch diagnostic.
///
/// Parses `"module 'foo' does not match filename 'bar'"` → `Some(("foo", "bar"))`.
pub(crate) fn extract_mismatch_names(message: &str) -> Option<(&str, &str)> {
    let start1 = message.find("module '")? + "module '".len();
    let end1 = message[start1..].find('\'')?;
    let module_name = &message[start1..start1 + end1];

    let rest = &message[start1 + end1..];
    let start2 = rest.find("filename '")? + "filename '".len();
    let end2 = rest[start2..].find('\'')?;
    let file_stem = &rest[start2..start2 + end2];

    Some((module_name, file_stem))
}

/// A type reference found at the cursor position (text-based detection).
pub(crate) struct TypeRefAtCursor {
    pub(crate) name: String,
    pub(crate) module: Option<String>,
}

/// Built-in Erlang types that don't need a definition.
pub(crate) const BUILTIN_TYPES: &[&str] = &[
    "integer", "float", "atom", "binary", "list", "tuple", "map", "pid",
    "reference", "port", "term", "any", "none", "boolean", "string",
    "iolist", "number", "byte", "char", "timeout", "mfa", "module", "node",
    "pos_integer", "neg_integer", "non_neg_integer", "nonempty_list",
    "iodata", "bitstring", "fun", "no_return", "maybe_improper_list",
    "nonempty_improper_list", "nonempty_maybe_improper_list",
    "nonempty_string", "nil",
];

/// Detect a type reference at the given byte offset within a type attribute.
///
/// Returns `Some(TypeRefAtCursor)` if the offset is inside a `-spec`, `-type`,
/// `-opaque`, or `-callback` attribute and points to a type name identifier.
pub(crate) fn find_type_ref_at_offset(source: &str, offset: usize) -> Option<TypeRefAtCursor> {
    if offset >= source.len() {
        return None;
    }
    let bytes = source.as_bytes();

    // Find the line containing the offset
    let line_start = source[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);

    // Scan backwards to find the start of the attribute (may span multiple lines)
    let mut attr_start = line_start;
    // Look for a line starting with `-spec`, `-type`, `-opaque`, or `-callback`
    // within a reasonable range (type attributes can span many lines)
    let search_start = offset.saturating_sub(2000);
    let before = &source[search_start..offset];

    // Find the last attribute keyword before offset
    let attr_keywords = ["-spec", "-type", "-opaque", "-callback"];
    let mut found_attr = false;
    let mut best_pos = 0;
    for kw in &attr_keywords {
        if let Some(pos) = before.rfind(kw) {
            // Check this is at start of line (or start of text)
            let abs_pos = search_start + pos;
            let is_line_start = abs_pos == 0 || source.as_bytes()[abs_pos - 1] == b'\n';
            if is_line_start && pos >= best_pos {
                best_pos = pos;
                attr_start = abs_pos;
                found_attr = true;
            }
        }
    }
    if !found_attr {
        return None;
    }

    // Verify we haven't passed the terminating dot
    // Find the end of this attribute
    let attr_body_start = attr_start;
    let mut i = attr_body_start;
    let mut depth: i32 = 0;
    let mut attr_end = source.len();
    while i < source.len() {
        match bytes[i] {
            b'%' => {
                while i < source.len() && bytes[i] != b'\n' { i += 1; }
            }
            b'"' => {
                i += 1;
                while i < source.len() {
                    if bytes[i] == b'\\' { i += 2; }
                    else if bytes[i] == b'"' { i += 1; break; }
                    else { i += 1; }
                }
            }
            b'\'' => {
                i += 1;
                while i < source.len() {
                    if bytes[i] == b'\\' { i += 2; }
                    else if bytes[i] == b'\'' { i += 1; break; }
                    else { i += 1; }
                }
            }
            b'(' | b'{' | b'[' => { depth += 1; i += 1; }
            b')' | b'}' | b']' => { depth -= 1; i += 1; }
            b'.' if depth <= 0 => {
                attr_end = i + 1;
                break;
            }
            _ => { i += 1; }
        }
    }

    // Offset must be within the attribute
    if offset >= attr_end {
        return None;
    }

    // Find the identifier at/around the offset
    let mut id_start = offset;
    while id_start > 0 && (bytes[id_start - 1].is_ascii_alphanumeric() || bytes[id_start - 1] == b'_') {
        id_start -= 1;
    }
    let mut id_end = offset;
    while id_end < source.len() && (bytes[id_end].is_ascii_alphanumeric() || bytes[id_end] == b'_') {
        id_end += 1;
    }

    if id_start == id_end {
        return None;
    }
    let ident = &source[id_start..id_end];

    // Must start with a lowercase letter (type/function name, not variable)
    if !ident.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false) {
        return None;
    }

    // Skip Erlang keywords
    let keywords = ["fun", "when", "end", "of", "record"];
    if keywords.contains(&ident) {
        return None;
    }

    // Check if preceded by `module:` (remote type reference)
    let module = if id_start > 1 && bytes[id_start - 1] == b':' {
        let mod_end = id_start - 1;
        let mut mod_start = mod_end;
        while mod_start > 0 && (bytes[mod_start - 1].is_ascii_alphanumeric() || bytes[mod_start - 1] == b'_') {
            mod_start -= 1;
        }
        if mod_start < mod_end {
            let mod_name = &source[mod_start..mod_end];
            if mod_name.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false) {
                Some(mod_name.to_string())
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    Some(TypeRefAtCursor {
        name: ident.to_string(),
        module,
    })
}

/// Check whether a string is a valid Erlang variable name.
/// Must start with an uppercase letter or underscore, followed by alphanumeric or underscore.
pub(crate) fn is_valid_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@')
}

/// Check whether a string is a valid unquoted Erlang atom (suitable for a
/// function name). Must start with a lowercase letter, followed by
/// alphanumeric, underscore, or `@`.
pub(crate) fn is_valid_unquoted_atom(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@')
}
