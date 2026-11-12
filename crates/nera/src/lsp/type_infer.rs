//! Heuristic type inference for Erlang functions.
//!
//! Infers approximate type signatures from function clauses by analysing
//! patterns, guards and return expressions.  The inferred types are used
//! by the LSP to generate `-spec` stubs and contextual completions.

use nera_syntax::ast;
use super::util::span_text;

// ---------------------------------------------------------------------------
// Heuristic type representation
// ---------------------------------------------------------------------------

/// A lightweight, approximate Erlang type inferred from source patterns.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum HeuristicType {
    /// A specific atom literal, e.g. `ok`, `error`.
    Atom(String),
    Integer,
    Float,
    Number,
    Binary,
    /// Erlang `string()` (a list of characters).
    StringType,
    List,
    Pid,
    Boolean,
    Map,
    /// A tuple with element types, e.g. `{ok, integer()}`.
    Tuple(Vec<HeuristicType>),
    /// A record type, e.g. `#person{}`.
    Record(String),
    /// Union of multiple types.
    Union(Vec<HeuristicType>),
    /// Fallback / unknown — renders as `term()`.
    Term,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Infer a heuristic spec for `func`: returns `(param_types, return_type)`.
pub(crate) fn infer_function_spec(
    func: &ast::Function,
    source: &str,
) -> (Vec<HeuristicType>, HeuristicType) {
    let arity = func.arity as usize;
    let mut all_param_types: Vec<Vec<HeuristicType>> = vec![vec![]; arity];
    let mut all_return_types: Vec<HeuristicType> = Vec::new();

    for clause in &func.clauses {
        // Collect guard-inferred types: variable_name -> type
        let guard_map = clause
            .guard
            .as_ref()
            .map(|g| extract_guard_types(g, source))
            .unwrap_or_default();

        // Infer each parameter type from its pattern + guard refinement.
        for (i, pat) in clause.patterns.iter().enumerate() {
            let mut ty = infer_type_from_pattern(pat, source);

            // If the pattern is a bare variable, check if a guard refines it.
            if ty == HeuristicType::Term {
                if let ast::Pattern::Variable(_, span) = pat {
                    let var_name = span_text(source, *span);
                    if let Some(guard_ty) = guard_map.get(var_name) {
                        ty = guard_ty.clone();
                    }
                }
            }

            if i < arity {
                all_param_types[i].push(ty);
            }
        }

        // Infer return type from last expression in body.
        let ret = if let Some(last) = clause.body.last() {
            infer_type_from_expr(last, source, &clause.patterns)
        } else {
            HeuristicType::Term
        };
        all_return_types.push(ret);
    }

    let param_types: Vec<HeuristicType> = all_param_types
        .into_iter()
        .map(union_types)
        .collect();
    let return_type = union_types(all_return_types);

    (param_types, return_type)
}

/// Render a `HeuristicType` as Erlang type syntax.
pub(crate) fn format_type(ty: &HeuristicType) -> String {
    match ty {
        HeuristicType::Atom(name) => name.clone(),
        HeuristicType::Integer => "integer()".into(),
        HeuristicType::Float => "float()".into(),
        HeuristicType::Number => "number()".into(),
        HeuristicType::Binary => "binary()".into(),
        HeuristicType::StringType => "string()".into(),
        HeuristicType::List => "list()".into(),
        HeuristicType::Pid => "pid()".into(),
        HeuristicType::Boolean => "boolean()".into(),
        HeuristicType::Map => "map()".into(),
        HeuristicType::Term => "term()".into(),
        HeuristicType::Record(name) => format!("#{}{{}}",  name),
        HeuristicType::Tuple(elems) => {
            let inner: Vec<String> = elems.iter().map(format_type).collect();
            format!("{{{}}}", inner.join(", "))
        }
        HeuristicType::Union(types) => {
            let parts: Vec<String> = types.iter().map(format_type).collect();
            parts.join(" | ")
        }
    }
}

/// Format a complete `-spec` line.
pub(crate) fn format_spec(
    func_name: &str,
    param_types: &[HeuristicType],
    return_type: &HeuristicType,
) -> String {
    let params: Vec<String> = param_types.iter().map(format_type).collect();
    format!(
        "-spec {}({}) -> {}.",
        func_name,
        params.join(", "),
        format_type(return_type)
    )
}

/// Format a snippet string for LSP completion (with tabstops).
pub(crate) fn format_spec_snippet(
    func_name: &str,
    param_types: &[HeuristicType],
    return_type: &HeuristicType,
) -> String {
    let mut tab = 1u32;
    let params: Vec<String> = param_types
        .iter()
        .map(|ty| {
            let s = format!("${{{}:{}}}", tab, escape_snippet(&format_type(ty)));
            tab += 1;
            s
        })
        .collect();
    let ret = format!("${{{}:{}}}", tab, escape_snippet(&format_type(return_type)));
    format!("{}({}) -> {}.", func_name, params.join(", "), ret)
}

// ---------------------------------------------------------------------------
// Pattern → type
// ---------------------------------------------------------------------------

fn infer_type_from_pattern(pat: &ast::Pattern, source: &str) -> HeuristicType {
    match pat {
        ast::Pattern::Wildcard(_) => HeuristicType::Term,
        ast::Pattern::Variable(_, _) => HeuristicType::Term,
        ast::Pattern::Integer(_, _) | ast::Pattern::BigInteger(_, _) => HeuristicType::Integer,
        ast::Pattern::Float(_, _) => HeuristicType::Float,
        ast::Pattern::Atom(_, span) => {
            let text = span_text(source, *span);
            match text {
                "true" | "false" => HeuristicType::Boolean,
                _ => HeuristicType::Atom(text.to_string()),
            }
        }
        ast::Pattern::String(_, _) => HeuristicType::StringType,
        ast::Pattern::Tuple(pats, _) => {
            let elems: Vec<HeuristicType> =
                pats.iter().map(|p| infer_type_from_pattern(p, source)).collect();
            HeuristicType::Tuple(elems)
        }
        ast::Pattern::Cons(_, _, _) | ast::Pattern::Nil(_) => HeuristicType::List,
        ast::Pattern::Binary(_, _) => HeuristicType::Binary,
        ast::Pattern::Map(_, _) => HeuristicType::Map,
        ast::Pattern::Record { span, .. } => {
            // Extract record name from source text: "#name{..."
            let text = span_text(source, *span);
            let name = extract_record_name_from_text(text);
            HeuristicType::Record(name)
        }
        ast::Pattern::Match(p1, p2, _) => {
            let t1 = infer_type_from_pattern(p1, source);
            let t2 = infer_type_from_pattern(p2, source);
            // Pick the more specific one (not Term).
            if t1 != HeuristicType::Term {
                t1
            } else {
                t2
            }
        }
        ast::Pattern::Concat(_, _, _) => HeuristicType::StringType,
        ast::Pattern::MacroCall { .. } => HeuristicType::Term,
    }
}

// ---------------------------------------------------------------------------
// Expression → type (for return type inference)
// ---------------------------------------------------------------------------

fn infer_type_from_expr(
    expr: &ast::Expr,
    source: &str,
    clause_patterns: &[ast::Pattern],
) -> HeuristicType {
    match expr {
        ast::Expr::Integer(_, _) | ast::Expr::BigInteger(_, _) => HeuristicType::Integer,
        ast::Expr::Float(_, _) => HeuristicType::Float,
        ast::Expr::Atom(_, span) => {
            let text = span_text(source, *span);
            match text {
                "true" | "false" => HeuristicType::Boolean,
                _ => HeuristicType::Atom(text.to_string()),
            }
        }
        ast::Expr::String(_, _) => HeuristicType::StringType,
        ast::Expr::Nil(_) => HeuristicType::List,
        ast::Expr::Cons(_, _, _) => HeuristicType::List,
        ast::Expr::Binary(_, _) => HeuristicType::Binary,
        ast::Expr::Map(_, _) | ast::Expr::MapUpdate(_, _, _) => HeuristicType::Map,
        ast::Expr::Tuple(exprs, _) => {
            let elems: Vec<HeuristicType> = exprs
                .iter()
                .map(|e| infer_type_from_expr(e, source, clause_patterns))
                .collect();
            HeuristicType::Tuple(elems)
        }
        ast::Expr::Variable(_, span) => {
            // Try to find this variable in the clause patterns.
            let var_name = span_text(source, *span);
            find_variable_type_in_patterns(var_name, clause_patterns, source)
        }
        ast::Expr::Case(_, clauses, _) => {
            let types: Vec<HeuristicType> = clauses
                .iter()
                .filter_map(|c| c.body.last())
                .map(|e| infer_type_from_expr(e, source, clause_patterns))
                .collect();
            union_types(types)
        }
        ast::Expr::If(clauses, _) => {
            let types: Vec<HeuristicType> = clauses
                .iter()
                .filter_map(|c| c.body.last())
                .map(|e| infer_type_from_expr(e, source, clause_patterns))
                .collect();
            union_types(types)
        }
        ast::Expr::Block(exprs, _) => {
            if let Some(last) = exprs.last() {
                infer_type_from_expr(last, source, clause_patterns)
            } else {
                HeuristicType::Term
            }
        }
        ast::Expr::Match(_, rhs, _) => {
            infer_type_from_expr(rhs, source, clause_patterns)
        }
        ast::Expr::RecordCreate { span, .. } => {
            let text = span_text(source, *span);
            let name = extract_record_name_from_text(text);
            HeuristicType::Record(name)
        }
        ast::Expr::Call(target, _, _) => {
            // Recognise a few well-known return types from BIFs.
            infer_call_return_type(target, source)
        }
        _ => HeuristicType::Term,
    }
}

/// Look up a variable name in the clause patterns to get its type.
fn find_variable_type_in_patterns(
    var_name: &str,
    patterns: &[ast::Pattern],
    source: &str,
) -> HeuristicType {
    for pat in patterns {
        if let Some(ty) = find_var_in_pattern(var_name, pat, source) {
            return ty;
        }
    }
    HeuristicType::Term
}

/// Recursively search a pattern tree for a variable binding.
fn find_var_in_pattern(
    var_name: &str,
    pat: &ast::Pattern,
    source: &str,
) -> Option<HeuristicType> {
    match pat {
        ast::Pattern::Variable(_, span) => {
            if span_text(source, *span) == var_name {
                // Found the variable, but a bare variable tells us nothing.
                // Return None so the caller keeps searching (e.g. in Match patterns).
                None
            } else {
                None
            }
        }
        ast::Pattern::Match(p1, p2, _) => {
            // In `X = {ok, V}`, if we're looking for X, return the type of p2.
            if matches!(p1.as_ref(), ast::Pattern::Variable(_, span) if span_text(source, *span) == var_name)
            {
                return Some(infer_type_from_pattern(p2, source));
            }
            if matches!(p2.as_ref(), ast::Pattern::Variable(_, span) if span_text(source, *span) == var_name)
            {
                return Some(infer_type_from_pattern(p1, source));
            }
            // Recurse into both sides.
            find_var_in_pattern(var_name, p1, source)
                .or_else(|| find_var_in_pattern(var_name, p2, source))
        }
        ast::Pattern::Tuple(pats, _) => {
            for p in pats {
                if let Some(ty) = find_var_in_pattern(var_name, p, source) {
                    return Some(ty);
                }
            }
            None
        }
        ast::Pattern::Cons(head, tail, _) => {
            find_var_in_pattern(var_name, head, source)
                .or_else(|| find_var_in_pattern(var_name, tail, source))
        }
        ast::Pattern::Record { fields, .. } => {
            for f in fields {
                if let Some(ty) = find_var_in_pattern(var_name, &f.pattern, source) {
                    return Some(ty);
                }
            }
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Guard → type refinements
// ---------------------------------------------------------------------------

/// Extract variable type constraints from guard expressions.
/// Returns a map of variable_name -> HeuristicType.
fn extract_guard_types(
    guard: &ast::Guard,
    source: &str,
) -> std::collections::HashMap<String, HeuristicType> {
    let mut map = std::collections::HashMap::new();
    collect_guard_types(guard, source, &mut map);
    map
}

fn collect_guard_types(
    guard: &ast::Guard,
    source: &str,
    map: &mut std::collections::HashMap<String, HeuristicType>,
) {
    match guard {
        ast::Guard::Test(expr) => {
            check_guard_expr(expr, source, map);
        }
        ast::Guard::Conjunction(exprs) => {
            for expr in exprs {
                check_guard_expr(expr, source, map);
            }
        }
        ast::Guard::Disjunction(guards) => {
            // For disjunctions, we could take the union, but for simplicity
            // we only keep types that appear in ALL branches.
            // For now, just collect from all branches (optimistic).
            for g in guards {
                collect_guard_types(g, source, map);
            }
        }
    }
}

fn check_guard_expr(
    expr: &ast::Expr,
    source: &str,
    map: &mut std::collections::HashMap<String, HeuristicType>,
) {
    if let ast::Expr::Call(ast::CallTarget::Local(_, name_span), args, _) = expr {
        let func_name = span_text(source, *name_span);
        if args.len() == 1 {
            if let ast::Expr::Variable(_, var_span) = &args[0] {
                let var_name = span_text(source, *var_span).to_string();
                if let Some(ty) = guard_function_to_type(func_name) {
                    map.insert(var_name, ty);
                }
            }
        }
    }
}

fn guard_function_to_type(name: &str) -> Option<HeuristicType> {
    match name {
        "is_integer" => Some(HeuristicType::Integer),
        "is_float" => Some(HeuristicType::Float),
        "is_number" => Some(HeuristicType::Number),
        "is_atom" => Some(HeuristicType::Atom("atom()".into())),
        "is_list" => Some(HeuristicType::List),
        "is_binary" => Some(HeuristicType::Binary),
        "is_bitstring" => Some(HeuristicType::Binary),
        "is_pid" => Some(HeuristicType::Pid),
        "is_boolean" => Some(HeuristicType::Boolean),
        "is_map" => Some(HeuristicType::Map),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Call return type hints
// ---------------------------------------------------------------------------

fn infer_call_return_type(target: &ast::CallTarget, source: &str) -> HeuristicType {
    match target {
        ast::CallTarget::Local(_, span) => {
            let name = span_text(source, *span);
            match name {
                "self" => HeuristicType::Pid,
                "length" | "size" | "byte_size" | "bit_size" | "tuple_size"
                | "map_size" => HeuristicType::Integer,
                "is_atom" | "is_integer" | "is_float" | "is_list" | "is_binary"
                | "is_pid" | "is_map" | "is_boolean" | "is_number"
                | "is_tuple" | "is_function" | "is_reference" => HeuristicType::Boolean,
                "atom_to_list" | "integer_to_list" | "float_to_list"
                | "binary_to_list" | "pid_to_list" => HeuristicType::List,
                "list_to_atom" | "binary_to_existing_atom" => HeuristicType::Atom("atom()".into()),
                "list_to_integer" => HeuristicType::Integer,
                "list_to_float" | "binary_to_float" => HeuristicType::Float,
                "list_to_binary" | "atom_to_binary" | "integer_to_binary"
                | "float_to_binary" | "term_to_binary" => HeuristicType::Binary,
                "abs" => HeuristicType::Number,
                "hd" | "tl" => HeuristicType::Term,
                _ => HeuristicType::Term,
            }
        }
        _ => HeuristicType::Term,
    }
}

// ---------------------------------------------------------------------------
// Union simplification
// ---------------------------------------------------------------------------

/// Combine multiple types into one (possibly a Union), with deduplication.
pub(crate) fn union_types(types: Vec<HeuristicType>) -> HeuristicType {
    if types.is_empty() {
        return HeuristicType::Term;
    }

    // Flatten nested unions.
    let mut flat: Vec<HeuristicType> = Vec::new();
    for ty in types {
        match ty {
            HeuristicType::Union(inner) => flat.extend(inner),
            other => flat.push(other),
        }
    }

    // Deduplicate.
    let mut seen = Vec::new();
    for ty in flat {
        if !seen.contains(&ty) {
            seen.push(ty);
        }
    }

    // Simplify: true | false -> boolean().
    let has_true = seen.contains(&HeuristicType::Atom("true".into()));
    let has_false = seen.contains(&HeuristicType::Atom("false".into()));
    if has_true && has_false {
        seen.retain(|t| {
            t != &HeuristicType::Atom("true".into())
                && t != &HeuristicType::Atom("false".into())
        });
        if !seen.contains(&HeuristicType::Boolean) {
            seen.push(HeuristicType::Boolean);
        }
    }

    // If Term is present alongside other types, keep it as the union may be
    // informative (e.g. `integer() | term()` means "at least one branch is integer").
    // But if ALL types are Term, collapse.
    if seen.iter().all(|t| t == &HeuristicType::Term) {
        return HeuristicType::Term;
    }
    // Remove bare Term entries when other more specific types exist.
    if seen.len() > 1 {
        seen.retain(|t| t != &HeuristicType::Term);
    }

    match seen.len() {
        0 => HeuristicType::Term,
        1 => seen.into_iter().next().unwrap(),
        _ => HeuristicType::Union(seen),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the record name from source text like `#person{name = N, age = _}`.
fn extract_record_name_from_text(text: &str) -> String {
    // Skip the leading `#`, take chars until `{` or end.
    let after_hash = text.strip_prefix('#').unwrap_or(text);
    after_hash
        .chars()
        .take_while(|c| *c != '{' && !c.is_whitespace())
        .collect()
}

/// Escape `$` and `}` in a string for use inside an LSP snippet placeholder.
fn escape_snippet(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('$', "\\$")
        .replace('}', "\\}")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_type_atoms() {
        assert_eq!(format_type(&HeuristicType::Atom("ok".into())), "ok");
        assert_eq!(format_type(&HeuristicType::Integer), "integer()");
        assert_eq!(format_type(&HeuristicType::Boolean), "boolean()");
        assert_eq!(format_type(&HeuristicType::Term), "term()");
    }

    #[test]
    fn format_type_tuple() {
        let ty = HeuristicType::Tuple(vec![
            HeuristicType::Atom("ok".into()),
            HeuristicType::Term,
        ]);
        assert_eq!(format_type(&ty), "{ok, term()}");
    }

    #[test]
    fn format_type_union() {
        let ty = HeuristicType::Union(vec![
            HeuristicType::Atom("ok".into()),
            HeuristicType::Atom("error".into()),
        ]);
        assert_eq!(format_type(&ty), "ok | error");
    }

    #[test]
    fn format_type_record() {
        assert_eq!(
            format_type(&HeuristicType::Record("person".into())),
            "#person{}"
        );
    }

    #[test]
    fn format_spec_basic() {
        let spec = format_spec(
            "my_fun",
            &[HeuristicType::Integer, HeuristicType::Atom("ok".into())],
            &HeuristicType::Boolean,
        );
        assert_eq!(spec, "-spec my_fun(integer(), ok) -> boolean().");
    }

    #[test]
    fn format_spec_no_params() {
        let spec = format_spec("init", &[], &HeuristicType::Atom("ok".into()));
        assert_eq!(spec, "-spec init() -> ok.");
    }

    #[test]
    fn union_simplifies_true_false_to_boolean() {
        let u = union_types(vec![
            HeuristicType::Atom("true".into()),
            HeuristicType::Atom("false".into()),
        ]);
        assert_eq!(u, HeuristicType::Boolean);
    }

    #[test]
    fn union_deduplicates() {
        let u = union_types(vec![
            HeuristicType::Integer,
            HeuristicType::Integer,
            HeuristicType::Atom("ok".into()),
        ]);
        assert_eq!(
            u,
            HeuristicType::Union(vec![
                HeuristicType::Integer,
                HeuristicType::Atom("ok".into()),
            ])
        );
    }

    #[test]
    fn union_single_collapses() {
        let u = union_types(vec![HeuristicType::Integer]);
        assert_eq!(u, HeuristicType::Integer);
    }

    #[test]
    fn union_all_term_collapses() {
        let u = union_types(vec![HeuristicType::Term, HeuristicType::Term]);
        assert_eq!(u, HeuristicType::Term);
    }

    #[test]
    fn union_removes_term_when_specific_types_present() {
        let u = union_types(vec![HeuristicType::Integer, HeuristicType::Term]);
        assert_eq!(u, HeuristicType::Integer);
    }

    #[test]
    fn extract_record_name() {
        assert_eq!(extract_record_name_from_text("#person{name = N}"), "person");
        assert_eq!(extract_record_name_from_text("#state{}"), "state");
    }

    #[test]
    fn escape_snippet_special_chars() {
        assert_eq!(escape_snippet("term()"), "term()");
        assert_eq!(escape_snippet("{ok, $x}"), "{ok, \\$x\\}");
    }

    #[test]
    fn format_spec_snippet_basic() {
        let snip = format_spec_snippet(
            "foo",
            &[HeuristicType::Integer, HeuristicType::Atom("ok".into())],
            &HeuristicType::Boolean,
        );
        assert_eq!(snip, "foo(${1:integer()}, ${2:ok}) -> ${3:boolean()}.");
    }

    #[test]
    fn format_spec_snippet_no_params() {
        let snip = format_spec_snippet("init", &[], &HeuristicType::Atom("ok".into()));
        assert_eq!(snip, "init() -> ${1:ok}.");
    }
}
