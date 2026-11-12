//! Unit tests for compiler warning analysis.

use std::collections::HashMap;

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;
use nera_syntax::parser::Parser;

use super::{analyze_module, analyze_module_with_source};

/// Parse a module from source and return (Module, atom_names).
fn parse_module(src: &str) -> (ast::Module, HashMap<AtomId, String>) {
    let mut parser = Parser::new(src);
    let result = parser.parse_module();
    assert!(
        result.errors.is_empty(),
        "parse errors: {:#?}",
        result.errors
    );
    let atom_names = parser.atom_names();
    (result.module, atom_names)
}

fn warning_messages(warnings: &[Diagnostic]) -> Vec<String> {
    warnings.iter().map(|w| w.message.clone()).collect()
}

// ── Unused variable tests ────────────────────────────────

#[test]
fn unused_var_detected() {
    let src = r#"
        -module(test).
        f(X, Y) -> Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'X'") && m.contains("unused")),
        "expected unused warning for X, got: {:?}",
        msgs
    );
}

#[test]
fn underscore_prefix_not_warned() {
    let src = r#"
        -module(test).
        -export([f/2]).
        f(_X, Y) -> Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.is_empty(),
        "expected no warnings, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn wildcard_not_warned() {
    let src = r#"
        -module(test).
        -export([f/2]).
        f(_, Y) -> Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.is_empty(),
        "expected no warnings, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn var_used_in_guard_not_warned() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) when X > 0 -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.is_empty(),
        "expected no warnings, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn all_vars_used_no_warning() {
    let src = r#"
        -module(test).
        -export([f/2]).
        f(X, Y) -> X + Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.is_empty(),
        "expected no warnings, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn unused_var_in_case_clause() {
    let src = r#"
        -module(test).
        f(X) ->
            case X of
                {A, B} -> B;
                _ -> ok
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'A'") && m.contains("unused")),
        "expected unused warning for A, got: {:?}",
        msgs
    );
}

// ── Unreachable clause tests ─────────────────────────────

#[test]
fn unreachable_after_catch_all() {
    let src = r#"
        -module(test).
        f(X) ->
            case X of
                _ -> a;
                1 -> b
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("cannot be reached")),
        "expected unreachable warning, got: {:?}",
        msgs
    );
}

#[test]
fn guarded_clause_does_not_shadow() {
    let src = r#"
        -module(test).
        f(X) ->
            case X of
                Y when Y > 0 -> a;
                _ -> b
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("cannot be reached")),
        "did not expect unreachable warning, got: {:?}",
        msgs
    );
}

#[test]
fn unreachable_function_clause() {
    let src = r#"
        -module(test).
        f(_) -> a;
        f(1) -> b.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("cannot be reached")),
        "expected unreachable warning for function clause, got: {:?}",
        msgs
    );
}

#[test]
fn no_unreachable_with_guard() {
    let src = r#"
        -module(test).
        f(X) when X > 0 -> positive;
        f(_) -> other.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("cannot be reached")),
        "did not expect unreachable warning, got: {:?}",
        msgs
    );
}

/// Parse `f() -> {expr}.` beside `id/1` and return its diagnostics.
fn diagnostics_for_body(expr: &str) -> (String, Vec<Diagnostic>) {
    let source = format!("-module(test). -export([f/0]). f() -> {expr}. id(X) -> X.");
    let (module, names) = parse_module(&source);
    let diagnostics = analyze_module(&module, &names, None);
    (source, diagnostics)
}

/// The source text of each unreachable-clause warning.
fn unreachable_clauses<'a>(source: &'a str, diagnostics: &[Diagnostic]) -> Vec<&'a str> {
    diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("unreachable_clause"))
        .map(|d| {
            let span = d.span.expect("unreachable clause has a source location");
            &source[span.start..span.end]
        })
        .collect()
}

#[test]
fn bound_name_in_clause_pattern_is_a_comparison() {
    // A clause pattern naming an already-bound variable compares with it,
    // so the later clauses stay reachable and the binding is read. erlc
    // -Wall reports neither an unreachable clause nor an unused variable
    // for any of these.
    for expr in [
        "T = id(0), case id(0) of T -> matched; _ -> other end",
        "begin T = id(0) end, case id(0) of T -> matched; _ -> other end",
        "receive never -> ok after (T = id(0)) -> ok end,
            case id(0) of T -> matched; _ -> other end",
        "case T = id(0) of T -> matched; _ -> other end",
        "[case id(1) of X -> matched; _ -> other end || X <- [1, 2]]",
        "T = id(0), F = fun() -> case id(0) of T -> matched; _ -> other end end, F()",
        "if true -> T = id(0), case id(0) of T -> matched; _ -> other end end",
        "T = id(0), self() ! 1, receive T -> matched; _ -> other after 0 -> none end",
        "T = id(0), try id(0) of T -> matched; _ -> other catch _:_ -> err end",
        "T = id(0), try throw(0) catch T -> matched; _ -> other end",
        "begin T = id(0) end, T = id(0), ok",
        "receive never -> ok after (T = id(0)) -> ok end, T = id(0), ok",
    ] {
        let (source, diagnostics) = diagnostics_for_body(expr);
        assert!(diagnostics.is_empty(), "{source}\n{:?}", warning_messages(&diagnostics));
    }

    // A function parameter is bound in the body too.
    let source = "-module(test). -export([f/1]).
        f(T) -> case id(0) of T -> matched; _ -> other end. id(X) -> X.";
    let (module, names) = parse_module(source);
    let msgs = warning_messages(&analyze_module(&module, &names, None));
    assert!(msgs.is_empty(), "{source}\n{msgs:?}");
}

#[test]
fn fresh_name_after_bound_clause_still_matches_anything() {
    // erlc still warns here: X is fresh after the bound T clause, and a fun
    // head binds a fresh T that shadows the outer one.
    for (expr, unreachable) in [
        (
            "T = id(0), case id(1) of T -> matched; X -> {other, X}; _ -> never end",
            "_ -> never",
        ),
        ("T = id(0), F = fun(T) -> T; (_) -> b end, F(1)", "(_) -> b"),
    ] {
        let (source, diagnostics) = diagnostics_for_body(expr);
        assert_eq!(unreachable_clauses(&source, &diagnostics), [unreachable], "{source}");
    }
}

#[test]
fn catch_clause_class_limits_catch_all() {
    // A catch clause catches every exception only when its class is `_` or
    // a fresh variable. An omitted class means throw. erlc reports no
    // unreachable clause for these.
    for expr in [
        "try id(0) catch error:_ -> a; _:_ -> b end",
        "try id(0) catch X -> X; _:_ -> b end",
        "C = id(error), try id(0) catch C:R -> R; _:_ -> b end",
    ] {
        let (source, diagnostics) = diagnostics_for_body(expr);
        assert!(unreachable_clauses(&source, &diagnostics).is_empty(), "{source}");
    }

    // erlc warns once an earlier catch-all covers the later clause's class.
    for (expr, unreachable) in [
        ("try id(0) catch C:_ -> C; _:_ -> b end", "_:_ -> b"),
        ("try id(0) catch _:_ -> a; error:_ -> b end", "error:_ -> b"),
        ("try id(0) catch X -> X; _ -> b end", "_ -> b"),
        ("try id(0) catch error:X -> X; error:_ -> b end", "error:_ -> b"),
        ("T = id(0), try id(0) catch throw:_ -> a; T -> b end", "T -> b"),
    ] {
        let (source, diagnostics) = diagnostics_for_body(expr);
        assert_eq!(unreachable_clauses(&source, &diagnostics), [unreachable], "{source}");
    }
}

// ── Shadowed variable tests ─────────────────────────────

#[test]
fn shadow_in_fun_clause() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            F = fun(X) -> X + 1 end,
            F(X).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'X'") && m.contains("shadows")),
        "expected shadow warning for X in fun, got: {:?}",
        msgs
    );
}

#[test]
fn no_shadow_different_var_in_fun() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            F = fun(Y) -> Y + 1 end,
            F(X).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("shadows")),
        "did not expect shadow warning, got: {:?}",
        msgs
    );
}

#[test]
fn shadow_underscore_prefix_not_warned() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(_X) ->
            F = fun(_X) -> ok end,
            F(1).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("shadows")),
        "did not expect shadow warning for _X, got: {:?}",
        msgs
    );
}

#[test]
fn no_shadow_in_case_clause() {
    // case patterns do NOT create a new scope — they extend the
    // current scope via single-assignment match semantics.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            case X of
                X -> same;
                _ -> other
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("shadows")),
        "did not expect shadow warning in case clause, got: {:?}",
        msgs
    );
}

#[test]
fn shadow_in_list_comprehension() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            [X + 1 || X <- [1, 2, 3]].
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'X'") && m.contains("shadows") && m.contains("comprehension generator")),
        "expected shadow warning for X in LC, got: {:?}",
        msgs
    );
}

#[test]
fn shadow_nested_fun_in_fun() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            F = fun(Y) ->
                G = fun(X) -> X end,
                G(Y)
            end,
            F(X).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'X'") && m.contains("shadows")),
        "expected shadow warning for X in nested fun, got: {:?}",
        msgs
    );
}

#[test]
fn shadow_from_match_binding() {
    // X is bound in the body via `X = 1`, then shadowed in the fun.
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            X = 1,
            F = fun(X) -> X + 1 end,
            F(X).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'X'") && m.contains("shadows")),
        "expected shadow warning for X bound via match, got: {:?}",
        msgs
    );
}

// ── Unused function tests ───────────────────────────────

#[test]
fn unused_function_detected() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> ok.
        g() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'g/0'") && m.contains("unused")),
        "expected unused function warning for g/0, got: {:?}",
        msgs
    );
}

#[test]
fn exported_function_not_warned() {
    let src = r#"
        -module(test).
        -export([f/0, g/0]).
        f() -> ok.
        g() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("unused") && m.contains("function")),
        "did not expect unused function warning, got: {:?}",
        msgs
    );
}

#[test]
fn locally_called_function_not_warned() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> g().
        g() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("'g/0'") && m.contains("unused")),
        "did not expect unused warning for g/0, got: {:?}",
        msgs
    );
}

#[test]
fn fun_ref_counts_as_call() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> fun g/1.
        g(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("'g/1'") && m.contains("unused")),
        "did not expect unused warning for g/1 (referenced via fun), got: {:?}",
        msgs
    );
}

#[test]
fn multiple_unused_functions() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> ok.
        g() -> ok.
        h(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'g/0'") && m.contains("unused")),
        "expected unused warning for g/0, got: {:?}",
        msgs
    );
    assert!(
        msgs.iter().any(|m| m.contains("'h/1'") && m.contains("unused")),
        "expected unused warning for h/1, got: {:?}",
        msgs
    );
}

// ── Repeated variable tests ─────────────────────────────

#[test]
fn repeated_var_in_pattern_not_unused() {
    // Idiomatic Erlang: Socket appears twice for equality matching.
    // Neither occurrence should be flagged as unused.
    let src = r#"
        -module(test).
        -export([handle/2]).
        -record(state, {socket}).
        handle({tcp, Socket, _RawLine}, #state{socket=Socket} = _StateData) -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("'Socket'") && m.contains("unused")),
        "Socket should not be flagged as unused (repeated variable = equality check), got: {:?}",
        msgs
    );
}

#[test]
fn repeated_var_across_multiple_args_not_unused() {
    // X appears in both arguments — equality constraint, not unused.
    let src = r#"
        -module(test).
        -export([f/2]).
        f(X, X) -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("'X'") && m.contains("unused")),
        "X should not be flagged as unused (repeated = equality check), got: {:?}",
        msgs
    );
}

#[test]
fn single_var_still_unused() {
    // X appears only once and isn't used in the body — should still warn.
    let src = r#"
        -module(test).
        f(X, Y) -> Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'X'") && m.contains("unused")),
        "expected unused warning for X, got: {:?}",
        msgs
    );
}

#[test]
fn repeated_var_not_catch_all() {
    // foo(X, X) is NOT a catch-all — it requires both args to be equal.
    // The second clause should NOT be flagged as unreachable.
    let src = r#"
        -module(test).
        -export([foo/2]).
        foo(X, X) -> X;
        foo(_, _) -> other.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("cannot be reached")),
        "second clause should be reachable (first has repeated var constraint), got: {:?}",
        msgs
    );
}

#[test]
fn distinct_vars_still_catch_all() {
    // foo(X, Y) IS a catch-all — distinct variables, no constraint.
    let src = r#"
        -module(test).
        -export([foo/2]).
        foo(X, Y) -> {X, Y};
        foo(_, _) -> other.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("cannot be reached")),
        "second clause should be unreachable (first is true catch-all), got: {:?}",
        msgs
    );
}

#[test]
fn repeated_var_in_case_not_unused() {
    // Repeated variable inside a case pattern — equality constraint.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(Input) ->
            case Input of
                {X, X} -> equal;
                _ -> different
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("'X'") && m.contains("unused")),
        "X should not be flagged as unused (repeated in case pattern), got: {:?}",
        msgs
    );
}

#[test]
fn match_lhs_var_counts_as_use() {
    // B is bound in the function head, then used in `B = Expected` (equality
    // assertion).  The match LHS must count as a reference so B is not falsely
    // flagged as unused.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(B) ->
            Expected = 42,
            B = Expected,
            ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("'B'") && m.contains("unused")),
        "B should not be flagged as unused (used in match LHS for equality check), got: {:?}",
        msgs
    );
}

#[test]
fn repeated_var_in_fun_clause_not_catch_all() {
    // fun(X, X) is NOT a catch-all.
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            F = fun(X, X) -> X;
                   (_, _) -> other
                end,
            F(1, 2).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("cannot be reached")),
        "second fun clause should be reachable (first has repeated var), got: {:?}",
        msgs
    );
}

// ── Unused body-match variable tests ────────────────────

#[test]
fn unused_body_match_var() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            X = 1,
            ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'X'") && m.contains("unused")),
        "expected unused warning for X in body match, got: {:?}",
        msgs
    );
}

#[test]
fn used_body_match_var() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            X = 1,
            X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("'X'") && m.contains("unused")),
        "did not expect unused warning for X (used in body), got: {:?}",
        msgs
    );
}

#[test]
fn unused_body_match_var_underscore_prefix() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            _X = 1,
            ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("'_X'") && m.contains("unused")),
        "did not expect unused warning for _X, got: {:?}",
        msgs
    );
}

#[test]
fn unused_body_match_tuple_destructure() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            {X, Y} = {1, 2},
            X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'Y'") && m.contains("unused")),
        "expected unused warning for Y, got: {:?}",
        msgs
    );
    assert!(
        !msgs.iter().any(|m| m.contains("'X'") && m.contains("unused")),
        "did not expect unused warning for X, got: {:?}",
        msgs
    );
}

#[test]
fn chained_match_unused() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            X = Y = 1,
            X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'Y'") && m.contains("unused")),
        "expected unused warning for Y in chained match, got: {:?}",
        msgs
    );
    assert!(
        !msgs.iter().any(|m| m.contains("'X'") && m.contains("unused")),
        "did not expect unused warning for X in chained match, got: {:?}",
        msgs
    );
}

#[test]
fn body_match_var_used_in_fun() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            X = 1,
            F = fun() -> X end,
            F().
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("'X'") && m.contains("unused")),
        "X is used inside fun, should not warn, got: {:?}",
        msgs
    );
}

#[test]
fn body_match_equality_not_double_warned() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            X = 1,
            X = 1,
            ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("'X'") && m.contains("unused")),
        "X used for equality check should not warn, got: {:?}",
        msgs
    );
}

// ── Outer-scope var in nested pattern tests (Bug #7) ────

#[test]
fn body_var_used_in_receive_pattern() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            Pid = self(),
            receive {Pid, Result} -> Result end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("'Pid'") && m.contains("unused")),
        "Pid used in receive pattern should not warn, got: {:?}",
        msgs
    );
}

#[test]
fn body_var_used_in_case_pattern() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(Input) ->
            Expected = 42,
            case Input of
                {Expected, Value} -> Value;
                _ -> error
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("'Expected'") && m.contains("unused")),
        "Expected used in case pattern should not warn, got: {:?}",
        msgs
    );
}

#[test]
fn body_var_unused_not_in_pattern() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            X = 1,
            receive {Y} -> Y end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'X'") && m.contains("unused")),
        "X not used anywhere should still warn, got: {:?}",
        msgs
    );
}

// ── Unbound variable tests ──────────────────────────────

#[test]
fn unbound_var_in_body() {
    let src = r#"
        -module(test).
        f() -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'X'") && m.contains("unbound")),
        "expected unbound error for X, got: {:?}",
        msgs
    );
}

#[test]
fn bound_var_no_error() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("unbound")),
        "did not expect unbound error, got: {:?}",
        msgs
    );
}

#[test]
fn match_binding_in_scope() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> X = 1, X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("unbound")),
        "did not expect unbound error for match-bound X, got: {:?}",
        msgs
    );
}

#[test]
fn var_in_case_pattern_body() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            case X of
                Y -> Y
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("unbound")),
        "case pattern var Y should be in scope for body, got: {:?}",
        msgs
    );
}

#[test]
fn fun_captures_outer_scope() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            F = fun() -> X end,
            F().
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("unbound")),
        "fun should capture outer X, got: {:?}",
        msgs
    );
}

#[test]
fn unbound_var_in_fun() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            F = fun() -> Y end,
            F().
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'Y'") && m.contains("unbound")),
        "expected unbound error for Y in fun, got: {:?}",
        msgs
    );
}

#[test]
fn lc_generator_binding() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(L) -> [X || X <- L].
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("unbound")),
        "LC generator var X should be in scope, got: {:?}",
        msgs
    );
}

#[test]
fn receive_timeout_binding_read_after_receive() {
    // erlc accepts a read, after the receive, of a name its after timeout
    // binds. tests/receive_timeout_bindings.erl runs these reads natively
    // and on BEAM.
    for expr in [
        "receive never -> ok after (T = id(0)) -> ok end, T",
        "X = receive never -> ok after (T = id(5)) -> late end, {X, T}",
        "receive never -> ok after begin T = id(0), T end -> ok end, T",
        "receive never -> ok after (T = id(0)) -> ok end,
            G = fun(#{T := V}) -> V end, G(#{0 => key})",
    ] {
        let source = format!("-module(test). -export([f/0]). f() -> {expr}. id(X) -> X.");
        let (module, names) = parse_module(&source);
        let diagnostics = analyze_module(&module, &names, None);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.severity == nera_syntax::error::Severity::Error),
            "{source}\n{diagnostics:?}"
        );
    }

    // erlc still rejects a read of that name inside the after body.
    let source = "-module(test). -export([f/0]).
        f() -> receive never -> ok after (T = id(0)) -> T end. id(X) -> X.";
    let (module, names) = parse_module(source);
    let msgs = warning_messages(&analyze_module(&module, &names, None));
    assert_eq!(msgs, ["variable 'T' is unbound"], "{source}");

    // erlc reports the name unsafe after the receive when only some branches
    // may bind it again, even inside a block, a call argument, a nested
    // branch, a try or a maybe.
    for expr in [
        "receive a -> begin T = 1 end after (T = id(0)) -> ok end, T",
        "receive a -> _ = id(T = 1), ok after (T = id(0)) -> ok end, T",
        "receive never -> ok after (T = id(0)) -> begin T = 1 end end, T",
        "receive a -> case id(1) of 1 -> T = 1; _ -> ok end after (T = id(0)) -> ok end, T",
        "receive never -> ok after (T = id(0)) -> if 1 =:= 2 -> T = 1; true -> ok end end, T",
        "receive a -> receive b -> T = 1 after 0 -> ok end after (T = id(0)) -> ok end, T",
        "receive a -> try T = id(1) catch _:_ -> ok end after (T = id(0)) -> ok end, T",
        "receive a -> maybe T = id(1) end after (T = id(0)) -> ok end, T",
    ] {
        let source = format!("-module(test). -export([f/0]). f() -> {expr}. id(X) -> X.");
        let (module, names) = parse_module(&source);
        let diagnostics = analyze_module(&module, &names, None);
        assert!(
            diagnostics.iter().any(|d| d.severity == nera_syntax::error::Severity::Error
                && d.message.contains("'T'")),
            "{source}\n{diagnostics:?}"
        );
    }
}

// NERA-80: inputs to a fun/generator pattern are reads, not same-head bindings.
fn assert_unbound_pattern_input(source: &str, name: &str) {
    let (module, names) = parse_module(source);
    let diagnostics = analyze_module(&module, &names, None);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == nera_syntax::error::Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{source}\n{diagnostics:?}");
    assert_eq!(
        errors[0].message,
        format!("variable '{name}' is unbound"),
        "{source}"
    );
    let span = errors[0].span.expect("unbound read has a source location");
    assert_eq!(&source[span.start..span.end], name);
}

#[test]
fn fun_pattern_inputs_reject_same_head_bindings() {
    // NERA-80 / NERA-54 I-1: erlc rejects these, but lowering used nil/8-bit
    // defaults when the key/size's only binding came from the fun head.
    for (head, name) in [
        ("K, #{K := V}", "K"),
        ("#{K := V}, K", "K"),
        ("#{a := K, K := V}", "K"),
        ("{K, #{{route, K} := V}}", "K"),
        ("K = #{K := V}", "K"),
        ("[K, #{K := V}]", "K"),
        ("#r{key = K, value = #{K := V}}", "K"),
        ("N, <<V:N>>", "N"),
        ("#{a := N, b := <<V:(N + 1)>>}", "N"),
        ("<<N>>, <<V:N>>", "N"),
        ("<<V:N, N>>", "N"),
        ("<<V:V>>", "V"),
    ] {
        for prefix in ["fun", "fun Again"] {
            let source = format!(
                "-module(test). -export([f/0]).
                -record(r, {{key, value}}).
                f() -> {prefix}({head}) -> V end."
            );
            assert_unbound_pattern_input(&source, name);
        }
    }
}

#[test]
fn generator_pattern_inputs_reject_same_head_bindings() {
    // NERA-80: both comprehension forms and both generator variants share
    // the input-before-bindings rule, including nested keys and sizes.
    for (pattern, source, op, name) in [
        ("{K, #{K := V}}", "Inputs", "<-", "K"),
        ("{K, #{{route, K} := V}}", "Inputs", "<-", "K"),
        ("#{a := K, K := V}", "Inputs", "<-", "K"),
        ("#r{key = K, value = #{K := V}}", "Inputs", "<-", "K"),
        ("{N, <<V:(N + 1)>>}", "Inputs", "<-", "N"),
        ("{<<N>>, <<V:N>>}", "Inputs", "<-", "N"),
        ("<<V:N, N>>", "Inputs", "<-", "N"),
        ("<<V:V>>", "Inputs", "<-", "V"),
        ("<<V:N, N>>", "Inputs", "<=", "N"),
        ("<<V:V>>", "Inputs", "<=", "V"),
    ] {
        for (open, close) in [("[V ||", "]"), ("<< <<V>> ||", ">>")] {
            let source = format!(
                "-module(test). -export([f/1]).
                -record(r, {{key, value}}).
                f(Inputs) -> {open} {pattern} {op} {source} {close}."
            );
            assert_unbound_pattern_input(&source, name);
        }
    }
}

#[test]
fn pattern_inputs_keep_outer_and_sequential_bindings() {
    // NERA-80 must preserve NERA-54's valid inputs. Named fun self references
    // are also in scope for keys/sizes, as confirmed with erlc.
    for expr in [
        "K = route, fun(K, #{K := V}) -> {K, V} end",
        "N = 8, fun(N, <<V:N>>) -> {N, V} end",
        "N = 8, fun(<<N>>, <<V:N>>) -> {N, V} end",
        "fun(<<N, V:N>>) -> V end",
        "fun Again(#{Again := V}) -> V end",
        "fun Again(N) when N > 0 -> Again(N - 1); Again(0) -> ok end",
        "[V || K <- [route], #{K := V} <- [#{route => 5}]]",
        "<< <<V>> || <<K>> <= <<1>>, #{K := V} <- [#{1 => 5}] >>",
        "[V || N <- [8], <<V:N>> <- [<<5>>]]",
        "<< <<V>> || <<N>> <= <<8>>, <<V:N>> <= <<5>> >>",
        "[V || <<N, V:N>> <- [<<8, 5>>]]",
        "[V || <<N, V:N>> <= <<8, 5>>]",
        "<< <<V>> || <<N, V:N>> <- [<<8, 5>>] >>",
        "<< <<V>> || <<N, V:N>> <= <<8, 5>> >>",
    ] {
        let source = format!("-module(test). -export([f/0]). f() -> {expr}.");
        let (module, names) = parse_module(&source);
        let diagnostics = analyze_module(&module, &names, None);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.severity == nera_syntax::error::Severity::Error),
            "{source}\n{diagnostics:?}"
        );
    }
}

#[test]
fn record_default_pattern_inputs_are_checked() {
    // NERA-80: record defaults are the same expressions outside a function;
    // even an unused or overridden default must reject same-head inputs.
    for (expr, name) in [
        ("fun(K, #{K := V}) -> V end", "K"),
        ("fun(N, <<V:N>>) -> V end", "N"),
        ("[V || {K, #{K := V}} <- []]", "K"),
        ("<< <<V>> || <<V:N, N>> <= <<5, 8>> >>", "N"),
    ] {
        for body in ["ok", "#r{}", "#r{value = ok}"] {
            let source = format!(
                "-module(test). -export([f/0]).
                -record(r, {{value = {expr}}}). f() -> {body}."
            );
            assert_unbound_pattern_input(&source, name);
        }
    }
    for expr in [
        "fun(<<N, V:N>>) -> V end",
        "[V || K <- [route], #{K := V} <- [#{route => 5}]]",
    ] {
        let source = format!(
            "-module(test). -export([f/0]).
            -record(r, {{value = {expr}}}). f() -> #r{{}}."
        );
        let (module, names) = parse_module(&source);
        let diagnostics = analyze_module(&module, &names, None);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.severity == nera_syntax::error::Severity::Error),
            "{source}\n{diagnostics:?}"
        );
    }
}

#[test]
fn pattern_inputs_keep_exported_bindings() {
    // NERA-80: the new input checks must not reject outer inputs that the
    // existing compiler and erlc accept after a block/branch or scrutinee.
    for expr in [
        "begin K = route end, fun(K, #{K := V}) -> V end",
        "case Mode of true -> K = route; false -> K = other end,
            fun(K, #{K := V}) -> V end",
        "if Mode -> N = 8; true -> N = 16 end,
            [V || {N, <<V:N>>} <- []]",
        "receive {key, K} -> ok after 0 -> K = route end,
            fun(K, #{K := V}) -> V end",
        "case K = route of _ -> fun(K, #{K := V}) -> V end end",
        // NERA-125 F2: the diagnostic pass accepts a plain body read.
        // tests/branch_export_reads.erl runs these reads natively.
        "case Mode of true -> K = route; false -> K = other end, K",
    ] {
        let source = format!("-module(test). -export([f/1]). f(Mode) -> {expr}.");
        let (module, names) = parse_module(&source);
        let diagnostics = analyze_module(&module, &names, None);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.severity == nera_syntax::error::Severity::Error),
            "{source}\n{diagnostics:?}"
        );
    }

    // Sibling tuple elements/call arguments do not supply inputs to a fun.
    for expr in [
        "{K = route, fun(K, #{K := V}) -> V end}",
        "g(K = route, fun(K, #{K := V}) -> V end)",
    ] {
        let source = format!(
            "-module(test). -export([f/0]). f() -> {expr}.
            g(A, B) -> {{A, B}}."
        );
        assert_unbound_pattern_input(&source, "K");
    }
}

#[test]
fn unbound_in_guard() {
    let src = r#"
        -module(test).
        f(X) ->
            case X of
                Y when Z > 0 -> Y
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'Z'") && m.contains("unbound")),
        "expected unbound error for Z in guard, got: {:?}",
        msgs
    );
}

// ── Diagnostic code tests ───────────────────────────────

fn warning_codes(warnings: &[Diagnostic]) -> Vec<Option<String>> {
    warnings.iter().map(|w| w.code.clone()).collect()
}

#[test]
fn unused_variable_has_code() {
    let src = r#"
        -module(test).
        f(X) -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("unused_variable")),
        "expected code 'unused_variable', got: {:?}",
        warning_codes(&ws)
    );
}

#[test]
fn unused_body_match_var_has_code() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            X = 1,
            ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("unused_variable")),
        "expected code 'unused_variable' for body match var, got: {:?}",
        warning_codes(&ws)
    );
}

#[test]
fn unreachable_clause_has_code() {
    let src = r#"
        -module(test).
        f(_) -> a;
        f(1) -> b.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("unreachable_clause")),
        "expected code 'unreachable_clause', got: {:?}",
        warning_codes(&ws)
    );
}

#[test]
fn shadowed_variable_has_code() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            F = fun(X) -> X + 1 end,
            F(X).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("shadowed_variable")),
        "expected code 'shadowed_variable', got: {:?}",
        warning_codes(&ws)
    );
}

#[test]
fn unused_function_has_code() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> ok.
        g() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("unused_function")),
        "expected code 'unused_function', got: {:?}",
        warning_codes(&ws)
    );
}

// ── Deprecated function tests ────────────────────────────

#[test]
fn deprecated_attribute_parsed() {
    let src = r#"
        -module(test).
        -export([main/0, old/1]).
        -deprecated([old/1]).
        main() -> ok.
        old(X) -> X.
    "#;
    let (module, _names) = parse_module(src);
    assert_eq!(module.deprecated.len(), 1);
    assert_eq!(module.deprecated[0].arity, 1);
}

#[test]
fn deprecated_multiple_functions() {
    let src = r#"
        -module(test).
        -export([main/0, old/1, legacy/0]).
        -deprecated([old/1, legacy/0]).
        main() -> ok.
        old(X) -> X.
        legacy() -> done.
    "#;
    let (module, _names) = parse_module(src);
    assert_eq!(module.deprecated.len(), 2);
}

#[test]
fn deprecated_empty_list() {
    let src = r#"
        -module(test).
        -deprecated([]).
        f() -> ok.
    "#;
    let (module, _names) = parse_module(src);
    assert!(module.deprecated.is_empty());
}

#[test]
fn deprecated_call_warns() {
    let src = r#"
        -module(test).
        -export([main/0]).
        -deprecated([old/1]).
        main() -> old(42).
        old(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'old/1'") && m.contains("deprecated")),
        "expected deprecated warning for old/1, got: {:?}",
        msgs
    );
}

#[test]
fn deprecated_call_has_correct_code() {
    let src = r#"
        -module(test).
        -export([main/0]).
        -deprecated([old/1]).
        main() -> old(42).
        old(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("deprecated_function")),
        "expected code 'deprecated_function', got: {:?}",
        warning_codes(&ws)
    );
}

#[test]
fn deprecated_fun_ref_warns() {
    let src = r#"
        -module(test).
        -export([main/0]).
        -deprecated([old/1]).
        main() -> fun old/1.
        old(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("'old/1'") && m.contains("deprecated")),
        "expected deprecated warning for fun old/1 reference, got: {:?}",
        msgs
    );
}

#[test]
fn non_deprecated_call_no_warning() {
    let src = r#"
        -module(test).
        -export([main/0]).
        -deprecated([old/1]).
        main() -> helper().
        helper() -> ok.
        old(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("deprecated_function")),
        "expected no deprecated warnings for helper(), got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn deprecated_no_attribute_no_warning() {
    let src = r#"
        -module(test).
        -export([main/0]).
        main() -> helper().
        helper() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let deprecated_ws: Vec<_> = ws
        .iter()
        .filter(|w| w.code.as_deref() == Some("deprecated_function"))
        .collect();
    assert!(deprecated_ws.is_empty());
}

#[test]
fn deprecated_tuple_forms_warn() {
    for attribute in [
        "-deprecated({old,1}).",
        "-deprecated([{old,1}]).",
        "-deprecated([{old,1,\"use replacement. instead\"}]).",
        "-deprecated([{old,1,next_version}]).",
        "-deprecated([{old,1,next_major_release}]).",
        "-deprecated([{old,1,eventually}]).",
        "-deprecated([old/1, {legacy,0}]).",
    ] {
        let src = format!(
            "-module(test). {attribute} main() -> old(42), fun old/1. old(X) -> X. legacy() -> ok."
        );
        let (module, names) = parse_module(&src);
        let warnings = analyze_module(&module, &names, None);
        let warnings: Vec<_> = warnings
            .iter()
            .filter(|w| w.code.as_deref() == Some("deprecated_function"))
            .collect();
        assert_eq!(warnings.len(), 2, "{attribute}: {warnings:?}");
        assert!(warnings.iter().all(|w| w.message.contains("old/1")));
    }
}

#[test]
fn deprecated_wildcards_expand_only_exports() {
    for (attribute, expected) in [
        ("-deprecated({old,'_'}).", vec![("old", 0), ("old", 1)]),
        (
            "-deprecated(module).",
            vec![("old", 0), ("old", 1), ("other", 0)],
        ),
        (
            "-deprecated({'_','_',eventually}).",
            vec![("old", 0), ("old", 1), ("other", 0)],
        ),
    ] {
        // The export list follows the attribute: expansion must wait until
        // all attributes have been collected, and must exclude private arities.
        let src = format!("-module(test). {attribute} -export([old/0, old/1, other/0]). old() -> ok. old(X) -> X. old(X,Y) -> {{X,Y}}. other() -> ok.");
        let (module, names) = parse_module(&src);
        let actual: Vec<_> = module
            .deprecated
            .iter()
            .map(|f| (names[&f.name].as_str(), f.arity))
            .collect();
        assert_eq!(actual, expected, "{attribute}");
    }
}

#[test]
fn deprecated_repeated_attributes_preserve_arity_and_spans() {
    let src = "-module(test). -deprecated([old/1]). -deprecated({'quoted name',0,\"reason\"}). main() -> old(), old(1), fun 'quoted name'/0. old() -> ok. old(X) -> X. 'quoted name'() -> ok.";
    let (module, names) = parse_module(src);
    assert_eq!(module.deprecated.len(), 2);
    assert_eq!(
        &src[module.deprecated[0].span.start..module.deprecated[0].span.end],
        "old/1"
    );
    assert_eq!(
        &src[module.deprecated[1].span.start..module.deprecated[1].span.end],
        "{'quoted name',0,\"reason\"}"
    );
    let warnings = analyze_module(&module, &names, None);
    let warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w.code.as_deref() == Some("deprecated_function"))
        .collect();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings.iter().any(|w| w.message.contains("old/1")));
    assert!(warnings.iter().any(|w| w.message.contains("quoted name/0")));
}

// ── Unsafe variable tests ────────────────────────────────

#[test]
fn unsafe_var_in_case_some_branches() {
    // Y is bound in the first branch but not the second — using Y after
    // the case is unsafe.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            case X of
                1 -> Y = hello;
                2 -> ok
            end,
            Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("unsafe_variable")),
        "expected unsafe_variable error for Y, got: {:?}",
        msgs
    );
    assert!(
        msgs.iter().any(|m| m.contains("'Y'") && m.contains("unsafe") && m.contains("case")),
        "expected unsafe message mentioning Y and case, got: {:?}",
        msgs
    );
}

#[test]
fn safe_var_in_all_case_branches() {
    // Y is bound in every branch — safe to use after the case.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            case X of
                1 -> Y = hello;
                _ -> Y = world
            end,
            Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("unsafe_variable")),
        "should not warn when Y is bound in all branches, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn generator_pattern_refs_use_earlier_bindings() {
    // NERA-77 I-2: a later generator key/size reads a fresh earlier-generator
    // binding, not a same-named unsafe binding from the enclosing case.
    for (name, value, first, later) in [
        ("K", "route", "{K, _} <- [{route, 1}]", "#{K := V} <- Inputs"),
        ("N", "8", "{N, _} <- [{8, frame}]", "<<V:N, _/binary>> <- Inputs"),
        ("K", "1", "<<K>> <= <<1>>", "#{K := V} <- Inputs"),
    ] {
        for (open, close) in [("[V ||", "]"), ("<< <<V>> ||", ">>")] {
            let source = format!("-module(test). -export([f/2]). f(Mode, Inputs) ->
                case Mode of enabled -> {name} = {value}; disabled -> ok end,
                {open} {first}, {later} {close}.");
            let (module, names) = parse_module(&source);
            let warnings = analyze_module(&module, &names, None);
            assert!(!warnings.iter().any(|w| w.code.as_deref() == Some("unsafe_variable")),
                "earlier generator binding is safe: {source}\n{warnings:?}");
        }
    }

    // Do not hide a genuinely unsafe outer input, including a key read before
    // the generator that later shadows it.
    for qualifiers in [
        "#{K := V} <- Inputs",
        "#{K := V} <- Inputs, {K, _} <- [{route, 1}]",
    ] {
        let source = format!("-module(test). -export([f/2]). f(Mode, Inputs) ->
            case Mode of enabled -> K = route; disabled -> ok end,
            [V || {qualifiers}].");
        let (module, names) = parse_module(&source);
        let warnings = analyze_module(&module, &names, None);
        assert!(warnings.iter().any(|w| w.code.as_deref() == Some("unsafe_variable")),
            "outer input must remain unsafe: {source}\n{warnings:?}");
    }
}

/// Each construct binds K in only some branches, so K is unsafe after it.
const PARTIAL_K_BINDINGS: [&str; 4] = [
    "case X of on -> K = 1; off -> ok end",
    "if X > 0 -> K = 1; true -> ok end",
    "receive {ok, K} -> K; other -> ok after 0 -> ok end",
    "try X of on -> K = 1, K; _ -> ok catch _:_ -> ok end",
];

fn unsafe_k_diagnostics(binding: &str, later: &str) -> (String, Vec<Diagnostic>) {
    let source = format!("-module(test). -export([f/3]). f(X, L, B) -> {binding}, {later}.");
    let (module, names) = parse_module(&source);
    let diagnostics = analyze_module(&module, &names, None);
    (source, diagnostics)
}

#[test]
fn unsafe_var_rebound_by_fun_head_or_generator() {
    // Bug fix: a fun head, a named fun's name or a comprehension generator
    // binds a fresh K, so reading it there is not a read of the unsafe K.
    // erlc compiles each of these programs without an error.
    for later in [
        "[K || K <- L]",
        "<< <<K>> || <<K>> <= B >>",
        "[K || K <- L, K > 1]",
        "[{K, V} || {K, _} <- L, V <- [K]]",
        "F = fun(K) -> K end, F(1)",
        "F = fun(K) when K > 0 -> K end, F(1)",
        "F = fun K(0) -> 0; K(I) -> K(I - 1) end, F(1)",
        "F = fun K(#{K := V}) -> V end, F(#{})",
        "F = fun({K, _}) -> G = fun() -> K end, G() end, F({1, 2})",
    ] {
        for binding in PARTIAL_K_BINDINGS {
            let (source, diagnostics) = unsafe_k_diagnostics(binding, later);
            assert!(
                !diagnostics
                    .iter()
                    .any(|d| d.severity == nera_syntax::error::Severity::Error),
                "{source}\n{diagnostics:?}"
            );
        }
    }
}

#[test]
fn unsafe_var_read_inside_fun_or_comprehension() {
    // A read inside a fun or comprehension that does not rebind K still
    // reads the unsafe K: erlc reports "variable 'K' unsafe" for each.
    for later in [
        "F = fun(Y) -> K + Y end, F(1)",
        "F = fun(Y) when Y > K -> Y; (Y) -> Y end, F(1)",
        "F = fun({K}) -> K; (_) -> K end, F(1)",
        "F = fun(#{K := V}) -> V end, F(#{})",
        "F = fun() -> K = 2, K end, F()",
        "[{K, Y} || Y <- L]",
        "[Y || Y <- L, Y > K]",
        "[Y || Y <- K]",
        "[K || K <- K]",
        "{K, [K || K <- L]}",
    ] {
        for binding in PARTIAL_K_BINDINGS {
            let (source, diagnostics) = unsafe_k_diagnostics(binding, later);
            assert!(
                diagnostics.iter().any(|d| d.code.as_deref() == Some("unsafe_variable")
                    && d.message.starts_with("variable 'K' unsafe in")),
                "{source}\n{diagnostics:?}"
            );
        }
    }
}

#[test]
fn unsafe_var_in_if() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            if
                X > 0 -> Y = positive;
                true -> ok
            end,
            Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("unsafe_variable")),
        "expected unsafe_variable for Y in if, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn unsafe_var_in_receive() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            receive
                {ok, Y} -> Y;
                error -> ok
            after 1000 ->
                timeout
            end,
            Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("unsafe_variable")),
        "expected unsafe_variable for Y in receive, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn unsafe_var_not_used_later_no_warning() {
    // Y is bound in only one branch but never used after the case.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            case X of
                1 -> Y = hello, Y;
                2 -> ok
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("unsafe_variable")),
        "should not warn when Y is not used after the case, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn unsafe_var_underscore_prefix_no_warning() {
    // _Y is bound in only one branch but underscore-prefix suppresses.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            case X of
                1 -> _Y = hello;
                2 -> ok
            end,
            _Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("unsafe_variable")),
        "should not warn for _Y (underscore prefix), got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn unsafe_var_already_in_scope_no_warning() {
    // Y is already bound before the case — rebinding in a branch is just
    // a match assertion, not a new unsafe binding.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            Y = default,
            case X of
                1 -> Y = hello;
                2 -> ok
            end,
            Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("unsafe_variable")),
        "should not warn when Y is already in scope, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn unsafe_var_nested_case() {
    // Unsafe variable in a nested case inside a function body.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            case X of
                {a, Inner} ->
                    case Inner of
                        1 -> Z = one;
                        2 -> ok
                    end,
                    Z;
                _ -> ok
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("unsafe_variable")),
        "expected unsafe_variable for Z in nested case, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn unsafe_var_in_try_catch() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            try
                Y = compute()
            of
                _ -> ok
            catch
                _:_ -> ok
            end,
            Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    // Y is bound in the of clause path but not the catch path.
    // The unbound_vars pass will catch this as "unbound" since try body
    // bindings don't escape. The unsafe_vars pass may or may not fire
    // depending on whether of_clauses/catch_clauses introduce it.
    // At minimum we should not crash.
    let has_error = ws.iter().any(|w| {
        w.code.as_deref() == Some("unsafe_variable")
            || w.message.contains("unbound")
    });
    assert!(
        has_error,
        "expected some error for Y after try, got: {:?}",
        warning_messages(&ws)
    );
}

// ── Illegal guard expression tests ──────────────────────

#[test]
fn legal_guard_type_test() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) when is_integer(X) -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")
            || w.code.as_deref() == Some("illegal_guard_local_call")),
        "type test in guard should be legal, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn legal_guard_is_list() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) when is_list(X) -> ok;
        f(_) -> other.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")
            || w.code.as_deref() == Some("illegal_guard_local_call")),
        "is_list/1 in guard should be legal, got: {:?}",
        warning_messages(&ws)
    );
}

/// When atom_names is incomplete (e.g. due to incremental parse cache in
/// the LSP), guard BIF checks should silently skip unresolvable atoms
/// rather than emitting false "illegal in guard" errors.
#[test]
fn guard_check_skips_unresolvable_atoms() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) when is_list(X) -> ok;
        f(_) -> other.
    "#;
    let (module, full_names) = parse_module(src);

    // Build an incomplete atom_names that is MISSING the is_list atom.
    // This simulates the LSP scenario where cached ASTs have AtomIds
    // that don't exist in the current interner.
    let mut partial_names = full_names.clone();
    // Remove all atoms that aren't "test", "f", or "ok"/"other"
    // (keep module name and function name so the warning pass doesn't crash,
    // but remove "is_list" to trigger the bug).
    let is_list_id = full_names.iter()
        .find(|(_, v)| v.as_str() == "is_list")
        .map(|(k, _)| *k);
    if let Some(id) = is_list_id {
        partial_names.remove(&id);
    }

    let ws = analyze_module(&module, &partial_names, None);
    let guard_errors: Vec<_> = ws.iter()
        .filter(|w| w.code.as_deref() == Some("illegal_guard_local_call")
            || w.code.as_deref() == Some("illegal_guard_expr"))
        .collect();
    assert!(
        guard_errors.is_empty(),
        "unresolvable atom in guard should not produce false error, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn legal_guard_arithmetic() {
    let src = r#"
        -module(test).
        -export([f/2]).
        f(X, Y) when X + Y > 10 -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")
            || w.code.as_deref() == Some("illegal_guard_local_call")),
        "arithmetic in guard should be legal, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn legal_guard_bif_length() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) when length(X) > 0 -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")
            || w.code.as_deref() == Some("illegal_guard_local_call")),
        "length/1 in guard should be legal, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn legal_guard_remote_erlang_bif() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) when erlang:is_atom(X) -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")
            || w.code.as_deref() == Some("illegal_guard_local_call")),
        "erlang:is_atom/1 in guard should be legal, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn legal_guard_comparison_with_tuple() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) when X =:= {ok, done} -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")
            || w.code.as_deref() == Some("illegal_guard_local_call")),
        "tuple construction in guard comparison should be legal, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn legal_guard_boolean_ops() {
    let src = r#"
        -module(test).
        -export([f/2]).
        f(X, Y) when is_integer(X) andalso Y > 0 -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")
            || w.code.as_deref() == Some("illegal_guard_local_call")),
        "andalso in guard should be legal, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn illegal_guard_local_call() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) when my_check(X) -> ok.
        my_check(X) -> X > 0.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_local_call")),
        "local function call in guard should be illegal, got: {:?}",
        warning_messages(&ws)
    );
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("my_check/1") && m.contains("illegal")),
        "error message should mention function name, got: {:?}",
        msgs
    );
}

#[test]
fn illegal_guard_remote_non_bif() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) when io:format("~p", [X]) -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")),
        "remote non-guard call in guard should be illegal, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn illegal_guard_send() {
    let src = r#"
        -module(test).
        -export([f/2]).
        f(X, Y) when X ! Y -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")),
        "send in guard should be illegal, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn illegal_guard_case() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) when case X of 1 -> true; _ -> false end -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")),
        "case in guard should be illegal, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn illegal_guard_nested_in_legal() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) when check(X) =:= true -> ok.
        check(X) -> X > 0.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_local_call")),
        "nested illegal call inside legal guard expr should be caught, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn illegal_guard_match_operator() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) when Y = X -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")),
        "match operator in guard should be illegal, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn if_clause_guards_validated() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            if
                my_test(X) -> yes;
                true -> no
            end.
        my_test(X) -> X > 0.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_local_call")),
        "if clause guard with local call should be illegal, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn legal_guard_self_and_node() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() when self() =/= undefined -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")
            || w.code.as_deref() == Some("illegal_guard_local_call")),
        "self/0 in guard should be legal, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn legal_guard_map_get() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(M) when map_get(key, M) =:= value -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")
            || w.code.as_deref() == Some("illegal_guard_local_call")),
        "map_get/2 in guard should be legal, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn legal_guard_disjunction() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) when is_integer(X); is_float(X) -> number.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_expr")
            || w.code.as_deref() == Some("illegal_guard_local_call")),
        "guard disjunction with legal tests should be legal, got: {:?}",
        warning_messages(&ws)
    );
}

// ── Undefined export tests ───────────────────────────────

#[test]
fn exported_and_defined_no_error() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("undefined_export")),
        "expected no undefined_export error, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn exported_function_not_defined() {
    let src = r#"
        -module(test).
        -export([foo/1]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("undefined_export")
            && w.message.contains("foo/1")),
        "expected undefined_export error for foo/1, got: {:?}",
        msgs
    );
}

#[test]
fn exported_wrong_arity() {
    let src = r#"
        -module(test).
        -export([f/2]).
        f(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("undefined_export")
            && w.message.contains("f/2")),
        "expected undefined_export error for f/2, got: {:?}",
        msgs
    );
}

#[test]
fn multiple_undefined_exports() {
    let src = r#"
        -module(test).
        -export([a/0, b/1]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let undef_count = ws.iter()
        .filter(|w| w.code.as_deref() == Some("undefined_export"))
        .count();
    assert_eq!(undef_count, 2, "expected 2 undefined_export errors, got: {:?}", warning_messages(&ws));
}

#[test]
fn no_exports_no_errors() {
    let src = r#"
        -module(test).
        f() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("undefined_export")),
        "expected no undefined_export error, got: {:?}",
        warning_messages(&ws)
    );
}

// ── Duplicate function definition tests ─────────────────

#[test]
fn duplicate_function_single_def_no_error() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("duplicate_function")),
        "expected no duplicate_function error, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn duplicate_function_same_name_arity() {
    let src = r#"
        -module(test).
        f(1) -> one.
        f(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let dup_msgs: Vec<_> = ws
        .iter()
        .filter(|w| w.code.as_deref() == Some("duplicate_function"))
        .collect();
    assert_eq!(
        dup_msgs.len(),
        1,
        "expected 1 duplicate_function error, got: {:?}",
        warning_messages(&ws)
    );
    assert!(
        dup_msgs[0].message.contains("f/1"),
        "expected message about f/1, got: {}",
        dup_msgs[0].message
    );
}

#[test]
fn duplicate_function_same_name_different_arity_no_error() {
    let src = r#"
        -module(test).
        f(X) -> X.
        f(X, Y) -> X + Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("duplicate_function")),
        "expected no duplicate_function error, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn duplicate_function_multiple_duplicates() {
    let src = r#"
        -module(test).
        f(X) -> X.
        g(X) -> X.
        f(Y) -> Y.
        g(Y) -> Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let dup_msgs: Vec<_> = ws
        .iter()
        .filter(|w| w.code.as_deref() == Some("duplicate_function"))
        .collect();
    assert_eq!(
        dup_msgs.len(),
        2,
        "expected 2 duplicate_function errors, got: {:?}",
        warning_messages(&ws)
    );
}

// ── Binary segment validation tests ────────────────────────

#[test]
fn binary_segment_valid_integer() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> <<X:8>>.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let bin_errs: Vec<_> = ws
        .iter()
        .filter(|w| w.code.as_deref() == Some("invalid_binary_segment"))
        .collect();
    assert!(bin_errs.is_empty(), "expected no binary segment errors, got: {:?}", bin_errs);
}

#[test]
fn binary_segment_valid_float_32() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> <<X:32/float>>.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let bin_errs: Vec<_> = ws
        .iter()
        .filter(|w| w.code.as_deref() == Some("invalid_binary_segment"))
        .collect();
    assert!(bin_errs.is_empty(), "expected no binary segment errors, got: {:?}", bin_errs);
}

#[test]
fn binary_segment_invalid_float_size() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> <<X:3/float>>.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("invalid size") && m.contains("float")),
        "expected invalid float size error, got: {:?}",
        msgs
    );
}

#[test]
fn binary_segment_utf8_with_size() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> <<X:8/utf8>>.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("size not allowed") && m.contains("utf8")),
        "expected size-on-utf8 error, got: {:?}",
        msgs
    );
}

#[test]
fn binary_segment_float_signed_is_accepted_by_otp() {
    // NERA-112: OTP accepts signedness even when the type ignores it.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> <<X:32/float-signed>>.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.is_empty(),
        "expected no signedness-on-float error, got: {:?}",
        msgs
    );
}

#[test]
fn binary_segment_binary_signed_is_accepted_by_otp() {
    // NERA-112: OTP accepts signedness even when the type ignores it.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> <<X/binary-signed>>.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.is_empty(),
        "expected no signedness-on-binary error, got: {:?}",
        msgs
    );
}

#[test]
fn binary_segment_binary_little_is_accepted_by_otp() {
    // NERA-112: OTP accepts endianness even when the type ignores it.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> <<X:8/binary-little>>.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.is_empty(),
        "expected no endianness-on-binary error, got: {:?}",
        msgs
    );
}

#[test]
fn binary_segment_valid_integer_signed_little() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> <<X:16/integer-signed-little>>.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let bin_errs: Vec<_> = ws
        .iter()
        .filter(|w| w.code.as_deref() == Some("invalid_binary_segment"))
        .collect();
    assert!(bin_errs.is_empty(), "expected no binary segment errors, got: {:?}", bin_errs);
}

#[test]
fn binary_segment_pattern_invalid_float_size() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(<<X:3/float>>) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("invalid size") && m.contains("float")),
        "expected invalid float size error in pattern, got: {:?}",
        msgs
    );
}

#[test]
fn binary_specifier_conflicts_follow_otp_merge_order() {
    // NERA-112: real erlc OTP 29 readings; the new field precedes the old
    // one, aliases normalize to binary and can also declare a unit.
    for (list, category, new, previous) in [
        ("binary-utf8", "type", "utf8", "binary"),
        ("utf8-binary", "type", "binary", "utf8"),
        ("integer-float", "type", "float", "integer"),
        ("float-integer", "type", "integer", "float"),
        ("utf16-utf32", "type", "utf32", "utf16"),
        ("bytes-utf16", "type", "utf16", "binary"),
        ("bits-utf32", "type", "utf32", "binary"),
        ("signed-unsigned-utf8", "sign", "unsigned", "signed"),
        ("unsigned-signed", "sign", "signed", "unsigned"),
        ("big-little", "endianness", "little", "big"),
        ("native-big", "endianness", "big", "native"),
        ("unit:8-unit:16", "unit", "16", "8"),
        ("bits-unit:8", "unit", "8", "1"),
        ("unit:8-bits", "unit", "1", "8"),
        ("bytes-bits", "unit", "1", "8"),
        ("bits-bytes", "unit", "8", "1"),
        ("bytes-unit:1-utf8", "unit", "1", "8"),
        ("unit:1-bytes-utf8", "unit", "8", "1"),
        ("signed-binary-utf8", "type", "utf8", "binary"),
        ("utf8-signed-binary", "type", "binary", "utf8"),
        ("binary-utf8-integer", "type", "utf8", "binary"),
    ] {
        let expected = format!("conflict in {category} specification for bit field: '{new}' and '{previous}'");
        for body in [format!("f(X) -> <<X/{list}>>."), format!("f(<<X/{list}>>) -> X.")] {
            assert_eq!(error_messages(&body), vec![expected.clone()], "{body}");
        }
    }
}

#[test]
fn nonconflicting_binary_specifiers_are_accepted() {
    // NERA-112: signed UTF, ignored flags and identical declarations are
    // accepted by OTP 29, not conflicts with the implicit defaults.
    for list in [
        "utf8-signed", "signed-utf8", "utf16-signed-little", "utf32-big-signed",
        "utf8-unsigned-little", "integer-integer-signed-signed-big-big",
        "binary-bytes", "bytes-binary", "binary-bits", "bits-binary",
        "bits-bitstring-unit:1", "bytes-unit:8", "unit:1-binary",
    ] {
        for body in [format!("f(X) -> <<X/{list}>>."), format!("f(<<X/{list}>>) -> X.")] {
            assert_eq!(error_messages(&body), Vec::<String>::new(), "{body}");
        }
    }
    assert!(error_messages("f(X) -> <<X:1/unit:8-unit:8>>.").is_empty());
}

#[test]
fn string_pattern_checks_precede_specifier_conflicts_as_in_otp() {
    // NERA-112 extends NERA-106: good_string_size_type inspects the whole
    // list before merging, even if its final type is no longer a UTF type.
    let typed = "a literal string in a binary pattern must not have a type or a size";
    for list in ["integer-float", "signed-unsigned"] {
        assert_eq!(error_messages(&format!("f(<<\"ab\"/{list}>>) -> ok.")), vec![typed]);
    }
    for (list, new, previous) in [("binary-utf8", "utf8", "binary"), ("utf8-binary", "binary", "utf8")] {
        assert_eq!(error_messages(&format!("f(<<\"ab\"/{list}>>) -> ok.")),
            vec![format!("conflict in type specification for bit field: '{new}' and '{previous}'")]);
        assert_eq!(error_messages(&format!("f(<<\"ab\":16/{list}>>) -> ok.")), vec![typed]);
    }
}

// ── Unsized binary segment ordering tests ──────────────────

#[test]
fn binary_segment_unsized_at_end_ok() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(<<X:8, Rest/binary>>) -> {X, Rest}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let bin_errs: Vec<_> = ws
        .iter()
        .filter(|w| w.code.as_deref() == Some("invalid_binary_segment"))
        .collect();
    assert!(bin_errs.is_empty(), "expected no binary segment errors, got: {:?}", bin_errs);
}

#[test]
fn binary_segment_unsized_not_at_end_pattern() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(<<Rest/binary, X:8>>) -> {Rest, X}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("binary segment without size") && m.contains("only allowed at the end")),
        "expected unsized-not-at-end error, got: {:?}",
        msgs
    );
}

#[test]
fn binary_segment_unsized_not_at_end_expr_is_legal() {
    // In binary CONSTRUCTION (expressions), unsized `/binary` segments are
    // legal anywhere — the "must be last" rule only applies to binary patterns
    // (for unambiguous decoding). No warning should be emitted.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> <<X/binary, 0:8>>.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let msgs = warning_messages(&ws);
    assert!(
        !msgs.iter().any(|m| m.contains("binary segment without size")),
        "unsized binary in expression should NOT produce a warning, got: {:?}",
        msgs
    );
}

// ── String literals in binary patterns (typed_literal_string) ──

/// The error messages of `-module(p).` followed by `body`, in order.
fn error_messages(body: &str) -> Vec<String> {
    let (module, names) = parse_module(&format!("-module(p). {body}"));
    analyze_module(&module, &names, None).into_iter()
        .filter(|d| d.severity == nera_syntax::error::Severity::Error)
        .map(|d| d.message).collect()
}

#[test]
fn string_pattern_segments_follow_otp_typed_literal_string() {
    // NERA-106: OTP 29 refuses a string literal in a binary pattern with a
    // size, or with a type list that names no UTF type. Nera dropped the size
    // and read "ab"/binary as rest captures, so these compiled with another
    // meaning. Expectations are erlc 29's errors for the same patterns.
    let typed = "a literal string in a binary pattern must not have a type or a size";
    for (pattern, expected) in [
        (r#"<<"ab":16, R/binary>>"#, vec![typed]),
        (r#"<<X, "ab"/binary>>"#, vec![typed]),
        // The refused segment gets no other segment error.
        (r#"<<"ab"/binary, X>>"#, vec![typed]),
        (r#"<<"ab":16/utf16-little>>"#, vec![typed]),
        // Explicit defaults are a type list too.
        (r#"<<"ab"/integer>>"#, vec![typed]),
        (r#"<<"ab"/big-unsigned>>"#, vec![typed]),
        (r#"<<"ab"/signed>>"#, vec![typed]),
        (r#"<<"ab"/unit:8>>"#, vec![typed]),
        (r#"<<"":8>>"#, vec![typed]),
        (r#"<<"ab":16, "cd"/bytes>>"#, vec![typed, typed]),
        (r#"<<"ab">>"#, vec![]),
        (r#"<<"ab", R/binary>>"#, vec![]),
        (r#"<<"ab"/utf8>>"#, vec![]),
        (r#"<<"ab"/utf16-little>>"#, vec![]),
        (r#"<<"ab"/little-utf32>>"#, vec![]),
    ] {
        assert_eq!(error_messages(&format!("f({pattern}) -> ok.")), expected, "{pattern}");
    }
}

#[test]
fn typed_string_segments_are_refused_in_every_pattern_position() {
    // NERA-106: OTP applies typed_literal_string wherever a pattern appears,
    // and a <= generator still gets its own unsized-field error. Binary
    // construction keeps the string's size and type.
    let typed = "a literal string in a binary pattern must not have a type or a size";
    let in_generator = "binary fields without size are not allowed in patterns of bit string generators";
    for (body, expected) in [
        (r#"f(B) -> case B of <<"ab":16>> -> ok; _ -> no end."#, vec![typed]),
        (r#"f(B) -> <<"ab"/signed, _/binary>> = B."#, vec![typed]),
        (r#"f() -> fun(<<"ab":16>>) -> ok end."#, vec![typed]),
        (r#"f() -> receive <<"ab"/little>> -> ok end."#, vec![typed]),
        (r#"f(B) -> try B of <<"ab"/bytes>> -> ok catch _:_ -> no end."#, vec![typed]),
        (r#"f() -> try ok catch throw:<<"ab":16>> -> ok end."#, vec![typed]),
        (r#"f(B) -> maybe <<"ab":16>> ?= B, ok end."#, vec![typed]),
        (r#"f(B) -> [X || <<"ab":16, X>> <= B]."#, vec![typed]),
        (r#"f(B) -> [ok || <<"ab"/binary>> <= B]."#, vec![typed, in_generator]),
        (r#"f(L) -> [X || <<"ab"/binary, X>> <- L]."#, vec![typed]),
        (r#"f({[<<"ab":16>> | _]}) -> ok."#, vec![typed]),
        (r#"f(#{k := <<"ab"/binary>>}) -> ok."#, vec![typed]),
        (r#"-record(r, {a = fun(<<"ab":16>>) -> ok end})."#, vec![typed]),
        (r#"f() -> <<"ab":16, "cd"/little, "ef"/signed>>."#, vec![]),
    ] {
        assert_eq!(error_messages(body), expected, "{body}");
    }
}

// ── Module name vs filename mismatch tests ─────────────────

#[test]
fn module_name_matches_filename() {
    let src = r#"
        -module(hello).
        -export([f/0]).
        f() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, Some("hello.erl"));
    let mismatch: Vec<_> = ws
        .iter()
        .filter(|w| w.code.as_deref() == Some("module_name_mismatch"))
        .collect();
    assert!(mismatch.is_empty(), "expected no mismatch warning, got: {:?}", mismatch);
}

#[test]
fn module_name_mismatch_warns() {
    let src = r#"
        -module(goodbye).
        -export([f/0]).
        f() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, Some("hello.erl"));
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("goodbye") && m.contains("hello")),
        "expected module name mismatch warning, got: {:?}",
        msgs
    );
    // Should be a warning, not an error.
    let mismatch_diags: Vec<_> = ws
        .iter()
        .filter(|w| w.code.as_deref() == Some("module_name_mismatch"))
        .collect();
    assert_eq!(mismatch_diags.len(), 1);
    assert_eq!(
        mismatch_diags[0].severity,
        nera_syntax::error::Severity::Warning
    );
    // Diagnostic span should cover just the module name atom, not the entire file.
    let span = mismatch_diags[0].span.unwrap();
    assert_eq!(&src[span.start..span.end], "goodbye");
}

#[test]
fn module_name_mismatch_with_path() {
    let src = r#"
        -module(foo).
        -export([f/0]).
        f() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, Some("/some/path/bar.erl"));
    let msgs = warning_messages(&ws);
    assert!(
        msgs.iter().any(|m| m.contains("foo") && m.contains("bar")),
        "expected mismatch warning with full path, got: {:?}",
        msgs
    );
}

#[test]
fn module_name_no_filename_no_warning() {
    let src = r#"
        -module(anything).
        -export([f/0]).
        f() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let mismatch: Vec<_> = ws
        .iter()
        .filter(|w| w.code.as_deref() == Some("module_name_mismatch"))
        .collect();
    assert!(mismatch.is_empty(), "expected no warning when filename is None");
}

/// Regression: if the atom table maps the module name AtomId to a wrong
/// string (e.g. "true" instead of the real name), the source cross-validation
/// should prevent a false-positive module_name_mismatch warning.
#[test]
fn module_name_mismatch_stale_atom_table_suppressed() {
    let src = r#"
        -module(my_module).
        -export([f/0]).
        f() -> ok.
    "#;
    let (module, names) = parse_module(src);

    // Tamper with the atom table: replace the real name with "true"
    let mut bad_names = names.clone();
    bad_names.insert(module.name, "true".to_string());

    // Without source cross-validation: would produce a false warning
    let ws_no_source = super::analyze_module(&module, &bad_names, Some("my_module.erl"));
    // The old path might see a mismatch (true != my_module), but with
    // source validation it should be suppressed.
    let ws_with_source = super::analyze_module_with_source(
        &module, &bad_names, Some("my_module.erl"), Some(src),
    );

    let mismatch_with_source: Vec<_> = ws_with_source
        .iter()
        .filter(|w| w.code.as_deref() == Some("module_name_mismatch"))
        .collect();
    assert!(
        mismatch_with_source.is_empty(),
        "source cross-validation should suppress false mismatch, got: {:?}",
        mismatch_with_source.iter().map(|w| &w.message).collect::<Vec<_>>()
    );

    // Without source, the old behavior would produce a warning
    let mismatch_no_source: Vec<_> = ws_no_source
        .iter()
        .filter(|w| w.code.as_deref() == Some("module_name_mismatch"))
        .collect();
    assert_eq!(
        mismatch_no_source.len(), 1,
        "without source validation, stale atom table should produce a mismatch"
    );
}

// ── Behaviour callback tests ─────────────────────────────

fn behaviour_warnings(warnings: &[Diagnostic]) -> Vec<&Diagnostic> {
    warnings
        .iter()
        .filter(|w| w.code.as_deref() == Some("missing_behaviour_callback"))
        .collect()
}

#[test]
fn behaviour_gen_server_all_callbacks_no_warning() {
    let src = r#"
        -module(test).
        -behaviour(gen_server).
        -export([init/1, handle_call/3, handle_cast/2]).
        init(_) -> {ok, #{}}.
        handle_call(_Req, _From, State) -> {reply, ok, State}.
        handle_cast(_Msg, State) -> {noreply, State}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let bws = behaviour_warnings(&ws);
    assert!(bws.is_empty(), "expected no behaviour warnings, got: {:?}", bws);
}

#[test]
fn behaviour_gen_server_missing_handle_cast() {
    let src = r#"
        -module(test).
        -behaviour(gen_server).
        -export([init/1, handle_call/3]).
        init(_) -> {ok, #{}}.
        handle_call(_Req, _From, State) -> {reply, ok, State}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let bws = behaviour_warnings(&ws);
    assert_eq!(bws.len(), 1, "expected 1 warning, got: {:?}", bws);
    assert!(bws[0].message.contains("handle_cast/2"), "got: {}", bws[0].message);
}

#[test]
fn behaviour_supervisor_missing_init() {
    let src = r#"
        -module(test).
        -behaviour(supervisor).
        -export([start_link/0]).
        start_link() -> supervisor:start_link(test, []).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let bws = behaviour_warnings(&ws);
    assert_eq!(bws.len(), 1);
    assert!(bws[0].message.contains("init/1"));
}

#[test]
fn behaviour_dual_behaviour_validates_both() {
    let src = r#"
        -module(test).
        -behaviour(gen_server).
        -behaviour(application).
        -export([init/1, handle_call/3, handle_cast/2, start/2, stop/1]).
        init(_) -> {ok, #{}}.
        handle_call(_R, _F, S) -> {reply, ok, S}.
        handle_cast(_M, S) -> {noreply, S}.
        start(_Type, _Args) -> ok.
        stop(_State) -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let bws = behaviour_warnings(&ws);
    assert!(bws.is_empty(), "expected no warnings for dual behaviour with all callbacks, got: {:?}", bws);
}

#[test]
fn behaviour_unknown_no_warning() {
    let src = r#"
        -module(test).
        -behaviour(my_custom).
        -export([f/0]).
        f() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let bws = behaviour_warnings(&ws);
    assert!(bws.is_empty(), "expected no warnings for unknown behaviour, got: {:?}", bws);
}

#[test]
fn behavior_american_spelling() {
    let src = r#"
        -module(test).
        -behavior(gen_server).
        -export([init/1, handle_call/3]).
        init(_) -> {ok, #{}}.
        handle_call(_R, _F, S) -> {reply, ok, S}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let bws = behaviour_warnings(&ws);
    assert_eq!(bws.len(), 1, "expected 1 warning for missing handle_cast/2, got: {:?}", bws);
    assert!(bws[0].message.contains("handle_cast/2"));
}

#[test]
fn behaviour_optional_callbacks_not_warned() {
    let src = r#"
        -module(test).
        -behaviour(gen_server).
        -export([init/1, handle_call/3, handle_cast/2]).
        init(_) -> {ok, #{}}.
        handle_call(_R, _F, S) -> {reply, ok, S}.
        handle_cast(_M, S) -> {noreply, S}.
    "#;
    // terminate/2, handle_info/2, code_change/3 are NOT required
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let bws = behaviour_warnings(&ws);
    assert!(bws.is_empty(), "optional callbacks should not trigger warnings, got: {:?}", bws);
}

// ── Match underscore variable tests ─────────────────────

fn match_underscore_warnings(warnings: &[Diagnostic]) -> Vec<&Diagnostic> {
    warnings
        .iter()
        .filter(|w| w.code.as_deref() == Some("match_underscore_var"))
        .collect()
}

#[test]
fn match_underscore_var_bound_in_param_reused_in_case() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(_X) ->
            case something() of
                _X -> matched
            end.
        something() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let muv = match_underscore_warnings(&ws);
    assert_eq!(muv.len(), 1, "expected 1 match_underscore_var warning, got: {:?}", muv);
    assert!(muv[0].message.contains("'_X'"), "got: {}", muv[0].message);
}

#[test]
fn match_underscore_var_bound_once_no_warning() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(_X) -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let muv = match_underscore_warnings(&ws);
    assert!(muv.is_empty(), "expected no match_underscore_var warning, got: {:?}", muv);
}

#[test]
fn match_underscore_var_different_clauses_no_warning() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f({a, _X}) -> one;
        f({b, _X}) -> two.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let muv = match_underscore_warnings(&ws);
    assert!(muv.is_empty(), "each clause has fresh scope, got: {:?}", muv);
}

#[test]
fn match_underscore_var_body_match_already_bound() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(_X) ->
            _X = something(),
            ok.
        something() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let muv = match_underscore_warnings(&ws);
    assert_eq!(muv.len(), 1, "expected 1 warning for body match, got: {:?}", muv);
}

#[test]
fn match_underscore_var_in_fun_no_warning() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(_X) ->
            F = fun(_X) -> ok end,
            F.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let muv = match_underscore_warnings(&ws);
    assert!(muv.is_empty(), "fun clauses start fresh scope, got: {:?}", muv);
}

#[test]
fn match_underscore_var_bare_wildcard_no_warning() {
    let src = r#"
        -module(test).
        -export([f/2]).
        f(_, _) -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let muv = match_underscore_warnings(&ws);
    assert!(muv.is_empty(), "bare _ should never warn, got: {:?}", muv);
}

#[test]
fn match_underscore_var_non_underscore_reuse_no_warning() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            case something() of
                X -> matched
            end.
        something() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let muv = match_underscore_warnings(&ws);
    assert!(muv.is_empty(), "non-underscore reuse is intentional match, got: {:?}", muv);
}

#[test]
fn match_underscore_var_in_function_head_across_params() {
    let src = r#"
        -module(test).
        -export([f/2]).
        f(_X, _X) -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let muv = match_underscore_warnings(&ws);
    assert_eq!(muv.len(), 1, "expected 1 warning for _X reused across params, got: {:?}", muv);
}

#[test]
fn match_underscore_var_in_lc_generator() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(_X) ->
            [Y || _X <- [1,2,3], Y <- [_X]].
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let muv = match_underscore_warnings(&ws);
    assert!(!muv.is_empty(), "expected warning for _X in generator pattern, got: {:?}", muv);
}

// ── Duplicate record / field tests ───────────────────────

fn redefine_record_errors(warnings: &[Diagnostic]) -> Vec<&Diagnostic> {
    warnings
        .iter()
        .filter(|w| w.code.as_deref() == Some("redefine_record"))
        .collect()
}

fn redefine_field_errors(warnings: &[Diagnostic]) -> Vec<&Diagnostic> {
    warnings
        .iter()
        .filter(|w| w.code.as_deref() == Some("redefine_field"))
        .collect()
}

#[test]
fn redefine_record_single_definition_no_error() {
    let src = r#"
        -module(test).
        -record(foo, {a, b}).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(redefine_record_errors(&ws).is_empty(), "expected no redefine_record error, got: {:?}", ws);
}

#[test]
fn redefine_record_duplicate_name() {
    let src = r#"
        -module(test).
        -record(foo, {a, b}).
        -record(foo, {x, y}).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = redefine_record_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 redefine_record error, got: {:?}", errs);
    assert!(errs[0].message.contains("foo"), "error should mention record name 'foo': {:?}", errs[0]);
}

#[test]
fn redefine_record_different_names_no_error() {
    let src = r#"
        -module(test).
        -record(foo, {a, b}).
        -record(bar, {x, y}).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(redefine_record_errors(&ws).is_empty(), "expected no redefine_record error, got: {:?}", ws);
}

#[test]
fn redefine_field_duplicate() {
    let src = r#"
        -module(test).
        -record(foo, {x, x}).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = redefine_field_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 redefine_field error, got: {:?}", errs);
    assert!(errs[0].message.contains("x"), "error should mention field name 'x': {:?}", errs[0]);
    assert!(errs[0].message.contains("foo"), "error should mention record name 'foo': {:?}", errs[0]);
}

#[test]
fn redefine_field_unique_fields_no_error() {
    let src = r#"
        -module(test).
        -record(foo, {a, b, c}).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(redefine_field_errors(&ws).is_empty(), "expected no redefine_field error, got: {:?}", ws);
}

#[test]
fn redefine_record_and_field_combo() {
    let src = r#"
        -module(test).
        -record(foo, {a, a}).
        -record(foo, {b}).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let rec_errs = redefine_record_errors(&ws);
    let field_errs = redefine_field_errors(&ws);
    assert_eq!(rec_errs.len(), 1, "expected 1 redefine_record, got: {:?}", rec_errs);
    assert_eq!(field_errs.len(), 1, "expected 1 redefine_field, got: {:?}", field_errs);
}

// ── Illegal map construction tests ───────────────────────

fn illegal_map_construction_errors(ws: &[Diagnostic]) -> Vec<&Diagnostic> {
    ws.iter()
        .filter(|d| d.code.as_deref() == Some("illegal_map_construction"))
        .collect()
}

#[test]
fn illegal_map_construction_valid_assoc() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> #{a => 1, b => 2}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        illegal_map_construction_errors(&ws).is_empty(),
        "expected no illegal_map_construction error, got: {:?}",
        ws
    );
}

#[test]
fn illegal_map_construction_exact_in_construction() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> #{a := 1}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = illegal_map_construction_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 illegal_map_construction, got: {:?}", errs);
}

#[test]
fn illegal_map_construction_update_allows_exact() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(M) -> M#{a := 1}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        illegal_map_construction_errors(&ws).is_empty(),
        "expected no illegal_map_construction error for update, got: {:?}",
        ws
    );
}

#[test]
fn illegal_map_construction_mixed_fields() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> #{a => 1, b := 2}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = illegal_map_construction_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 illegal_map_construction for b := 2, got: {:?}", errs);
}

#[test]
fn illegal_map_construction_nested() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> {ok, #{a := 1}}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = illegal_map_construction_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 illegal_map_construction in nested expr, got: {:?}", errs);
}

// ── Exported variable tests ──────────────────────────────

fn exported_var_warnings(ws: &[Diagnostic]) -> Vec<&Diagnostic> {
    ws.iter().filter(|w| w.code.as_deref() == Some("exported_var")).collect()
}

#[test]
fn exported_var_from_case() {
    // Y is bound in every branch and used after — exported warning.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            case X of
                1 -> Y = a;
                2 -> Y = b
            end,
            Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let evs = exported_var_warnings(&ws);
    assert_eq!(evs.len(), 1, "expected 1 exported_var for Y, got: {:?}", warning_messages(&ws));
    assert!(evs[0].message.contains("'Y'") && evs[0].message.contains("case"),
        "expected message mentioning Y and case, got: {}", evs[0].message);
}

#[test]
fn exported_var_not_used_no_warning() {
    // Y is bound in every branch but never used after — no warning.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            case X of
                1 -> Y = a, Y;
                2 -> Y = b, Y
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let evs = exported_var_warnings(&ws);
    assert!(evs.is_empty(), "should not warn when Y is not used after case, got: {:?}", warning_messages(&ws));
}

#[test]
fn exported_var_from_if() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            if
                X > 0 -> Z = 1;
                true -> Z = 2
            end,
            Z.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let evs = exported_var_warnings(&ws);
    assert_eq!(evs.len(), 1, "expected 1 exported_var for Z from if, got: {:?}", warning_messages(&ws));
    assert!(evs[0].message.contains("'Z'") && evs[0].message.contains("if"));
}

#[test]
fn exported_var_from_receive() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            receive
                msg -> R = ok
            after 1000 ->
                R = timeout
            end,
            R.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let evs = exported_var_warnings(&ws);
    assert_eq!(evs.len(), 1, "expected 1 exported_var for R from receive, got: {:?}", warning_messages(&ws));
    assert!(evs[0].message.contains("'R'") && evs[0].message.contains("receive"));
}

#[test]
fn exported_var_underscore_prefix_no_warning() {
    // _X bound in all branches and used after — but underscore suppresses.
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            case X of
                1 -> _Y = a;
                2 -> _Y = b
            end,
            _Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let evs = exported_var_warnings(&ws);
    assert!(evs.is_empty(), "underscore-prefix should suppress exported_var, got: {:?}", warning_messages(&ws));
}

#[test]
fn exported_var_already_in_scope_no_warning() {
    // Y is already bound before the case — rebinding in all branches is not "new".
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            Y = init,
            case X of
                1 -> Y = a;
                2 -> Y = b
            end,
            Y.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let evs = exported_var_warnings(&ws);
    assert!(evs.is_empty(), "should not warn when Y is already in scope, got: {:?}", warning_messages(&ws));
}

#[test]
fn exported_var_nested_in_fun() {
    // Exported var inside a fun body is detected in the nested scope.
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            fun(X) ->
                case X of
                    1 -> V = a;
                    2 -> V = b
                end,
                V
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let evs = exported_var_warnings(&ws);
    assert_eq!(evs.len(), 1, "expected 1 exported_var for V in nested fun, got: {:?}", warning_messages(&ws));
}

#[test]
fn exported_var_from_try() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            try ok of
                ok -> V = a
            catch
                _:_ -> V = b
            end,
            V.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let evs = exported_var_warnings(&ws);
    assert_eq!(evs.len(), 1, "expected 1 exported_var for V from try, got: {:?}", warning_messages(&ws));
    assert!(evs[0].message.contains("'V'") && evs[0].message.contains("try"));
}

// ── Unused record tests ─────────────────────────────────

fn unused_record_warnings(warnings: &[Diagnostic]) -> Vec<&Diagnostic> {
    warnings.iter().filter(|w| w.code.as_deref() == Some("unused_record")).collect()
}

#[test]
fn record_used_in_create_no_warning() {
    let src = r#"
        -module(test).
        -export([f/0]).
        -record(person, {name, age}).
        f() -> #person{name = "Alice", age = 30}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        unused_record_warnings(&ws).is_empty(),
        "record used in create should not warn, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn record_defined_but_unused() {
    let src = r#"
        -module(test).
        -export([f/0]).
        -record(person, {name, age}).
        f() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let rws = unused_record_warnings(&ws);
    assert_eq!(rws.len(), 1, "expected 1 unused_record warning, got: {:?}", warning_messages(&ws));
    assert!(rws[0].message.contains("'person'"));
}

#[test]
fn record_used_in_pattern_no_warning() {
    let src = r#"
        -module(test).
        -export([f/1]).
        -record(person, {name, age}).
        f(#person{name = N}) -> N.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        unused_record_warnings(&ws).is_empty(),
        "record used in pattern should not warn, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn record_used_in_update_no_warning() {
    let src = r#"
        -module(test).
        -export([f/1]).
        -record(person, {name, age}).
        f(R) -> R#person{name = "Bob"}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        unused_record_warnings(&ws).is_empty(),
        "record used in update should not warn, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn record_used_in_access_no_warning() {
    let src = r#"
        -module(test).
        -export([f/1]).
        -record(person, {name, age}).
        f(R) -> R#person.name.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        unused_record_warnings(&ws).is_empty(),
        "record used in field access should not warn, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn record_used_in_index_no_warning() {
    let src = r#"
        -module(test).
        -export([f/0]).
        -record(person, {name, age}).
        f() -> #person.name.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        unused_record_warnings(&ws).is_empty(),
        "record used in index should not warn, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn multiple_records_one_unused() {
    let src = r#"
        -module(test).
        -export([f/0]).
        -record(person, {name, age}).
        -record(address, {street, city}).
        f() -> #person{name = "Alice", age = 30}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let rws = unused_record_warnings(&ws);
    assert_eq!(rws.len(), 1, "expected 1 unused_record warning, got: {:?}", warning_messages(&ws));
    assert!(rws[0].message.contains("'address'"), "expected warning for 'address', got: {:?}", rws[0].message);
}

// ── Unused type tests ─────────────────────────────────

fn unused_type_warnings(warnings: &[Diagnostic]) -> Vec<&Diagnostic> {
    warnings.iter().filter(|w| w.code.as_deref() == Some("unused_type")).collect()
}

#[test]
fn type_used_in_spec_no_warning() {
    let src = r#"
        -module(test).
        -export([foo/0]).
        -type my_type() :: integer().
        -spec foo() -> my_type().
        foo() -> 42.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        unused_type_warnings(&ws).is_empty(),
        "type used in spec should not warn, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn type_exported_no_warning() {
    let src = r#"
        -module(test).
        -export([foo/0]).
        -export_type([my_type/0]).
        -type my_type() :: integer().
        foo() -> 42.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        unused_type_warnings(&ws).is_empty(),
        "exported type should not warn, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn type_unused_warns() {
    let src = r#"
        -module(test).
        -export([foo/0]).
        -type unused_type() :: atom().
        foo() -> ok.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let tws = unused_type_warnings(&ws);
    assert_eq!(tws.len(), 1, "expected 1 unused_type warning, got: {:?}", warning_messages(&ws));
    assert!(tws[0].message.contains("'unused_type/0'"));
}

#[test]
fn type_referenced_by_another_type_no_warning() {
    let src = r#"
        -module(test).
        -export([foo/0]).
        -export_type([outer/0]).
        -type inner() :: integer().
        -type outer() :: {ok, inner()}.
        foo() -> 42.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        unused_type_warnings(&ws).is_empty(),
        "type referenced by another type should not warn, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn multiple_types_only_unused_warns() {
    let src = r#"
        -module(test).
        -export([foo/0]).
        -type used_type() :: integer().
        -type unused_one() :: atom().
        -type unused_two() :: binary().
        -spec foo() -> used_type().
        foo() -> 42.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let tws = unused_type_warnings(&ws);
    assert_eq!(tws.len(), 2, "expected 2 unused_type warnings, got: {:?}", warning_messages(&ws));
}

#[test]
fn opaque_type_exported_no_warning() {
    let src = r#"
        -module(test).
        -export([foo/0]).
        -export_type([my_opaque/0]).
        -opaque my_opaque() :: integer().
        foo() -> 42.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        unused_type_warnings(&ws).is_empty(),
        "exported opaque type should not warn, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn type_used_in_callback_no_warning() {
    let src = r#"
        -module(test).
        -export([init/1]).
        -type state() :: #{key => value}.
        -callback init(term()) -> {ok, state()}.
        init(_) -> {ok, #{}}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        unused_type_warnings(&ws).is_empty(),
        "type used in callback should not warn, got: {:?}",
        warning_messages(&ws)
    );
}

// ── Attributes after functions ──────────────────────────────

#[test]
fn unused_record_after_function_warns() {
    let src = r#"
        -module(test).
        -export([foo/0]).
        foo() -> ok.
        -record(unused_rec, {field}).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let rws = unused_record_warnings(&ws);
    assert_eq!(rws.len(), 1, "expected unused_record warning for record after function");
    assert!(rws[0].message.contains("'unused_rec'"));
}

#[test]
fn unused_type_after_function_warns() {
    let src = r#"
        -module(test).
        -export([foo/0]).
        foo() -> ok.
        -type unused_t() :: atom().
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let tws = unused_type_warnings(&ws);
    assert_eq!(tws.len(), 1, "expected unused_type warning for type after function");
    assert!(tws[0].message.contains("'unused_t/0'"));
}

#[test]
fn spec_after_function_collects_type_refs() {
    let src = r#"
        -module(test).
        -export([foo/0]).
        -type my_t() :: integer().
        foo() -> 42.
        -spec foo() -> my_t().
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        unused_type_warnings(&ws).is_empty(),
        "type referenced in spec after function should not warn, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn interleaved_attrs_warnings_correct() {
    let src = r#"
        -module(test).
        -export([foo/0, bar/0]).
        -record(used_rec, {x}).
        foo() -> #used_rec{x = 1}.
        -record(unused_rec, {y}).
        -type used_t() :: integer().
        bar() -> 42.
        -spec bar() -> used_t().
        -type unused_t() :: atom().
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let rws = unused_record_warnings(&ws);
    assert_eq!(rws.len(), 1, "expected 1 unused record warning");
    assert!(rws[0].message.contains("'unused_rec'"));
    let tws = unused_type_warnings(&ws);
    assert_eq!(tws.len(), 1, "expected 1 unused type warning");
    assert!(tws[0].message.contains("'unused_t/0'"));
}

// ── collect_used_record_names tests ─────────────────────

#[test]
fn collect_used_record_names_finds_used_records() {
    let src = r#"
        -module(test).
        -export([f/0]).
        -record(person, {name, age}).
        -record(address, {street}).
        -record(unused_rec, {x}).
        f() -> #person{name = "Alice", age = 30}.
    "#;
    let (module, names) = parse_module(src);
    let used = super::unused_records::collect_used_record_names(&module, &names);
    assert!(used.contains("person"), "should find 'person' as used");
    assert!(!used.contains("address"), "'address' is not used");
    assert!(!used.contains("unused_rec"), "'unused_rec' is not used");
}

#[test]
fn collect_used_record_names_finds_pattern_and_update_usage() {
    let src = r#"
        -module(test).
        -export([f/1, g/1]).
        -record(person, {name, age}).
        -record(address, {street}).
        f(#person{name = N}) -> N.
        g(R) -> R#address{street = "Main St"}.
    "#;
    let (module, names) = parse_module(src);
    let used = super::unused_records::collect_used_record_names(&module, &names);
    assert!(used.contains("person"), "should find 'person' used in pattern");
    assert!(used.contains("address"), "should find 'address' used in update");
}

// ── Undefined function tests ────────────────────────────

#[test]
fn undefined_local_call_detected() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> bar(1).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("undefined_function")
            && w.message.contains("bar/1")),
        "expected undefined_function error for bar/1, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn undefined_local_call_wrong_arity() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> g(1, 2).
        g(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let undef: Vec<_> = ws.iter()
        .filter(|w| w.code.as_deref() == Some("undefined_function"))
        .collect();
    assert_eq!(undef.len(), 1, "expected 1 undefined_function error, got: {:?}", warning_messages(&ws));
    assert!(
        undef[0].message.contains("g/2"),
        "expected message about g/2, got: {}",
        undef[0].message
    );
    assert!(
        undef[0].hint.as_ref().is_some_and(|h| h.contains("g/1")),
        "expected hint about g/1, got: {:?}",
        undef[0].hint
    );
}

#[test]
fn defined_local_call_no_error() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> g(1).
        g(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("undefined_function")),
        "expected no undefined_function errors, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn bif_auto_import_no_error() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> length(X).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        !ws.iter().any(|w| w.code.as_deref() == Some("undefined_function")),
        "expected no undefined_function for BIF auto-import, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn undefined_fun_ref_detected() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> fun nonexistent/2.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        ws.iter().any(|w| w.code.as_deref() == Some("undefined_function")
            && w.message.contains("nonexistent/2")),
        "expected undefined_function error for fun ref, got: {:?}",
        warning_messages(&ws)
    );
}

#[test]
fn undefined_function_has_error_severity() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> missing().
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let undef: Vec<_> = ws.iter()
        .filter(|w| w.code.as_deref() == Some("undefined_function"))
        .collect();
    assert_eq!(undef.len(), 1);
    assert_eq!(undef[0].severity, nera_syntax::error::Severity::Error);
}

#[test]
fn undefined_function_duplicate_not_reported_twice() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() ->
            bar(1),
            bar(1).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let undef_count = ws.iter()
        .filter(|w| w.code.as_deref() == Some("undefined_function")
            && w.message.contains("bar/1"))
        .count();
    assert_eq!(undef_count, 1, "expected 1 report for bar/1, got: {}", undef_count);
}

// ── Undefined record field tests ────────────────────────

fn undefined_record_field_errors(ws: &[Diagnostic]) -> Vec<&Diagnostic> {
    ws.iter()
        .filter(|w| w.code.as_deref() == Some("undefined_record_field"))
        .collect()
}

#[test]
fn undefined_record_field_valid_access() {
    let src = r#"
        -module(test).
        -export([f/0]).
        -record(person, {name, age}).
        f() -> #person{name = "Alice", age = 30}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        undefined_record_field_errors(&ws).is_empty(),
        "expected no undefined_record_field error, got: {:?}",
        ws
    );
}

#[test]
fn undefined_record_field_in_create() {
    let src = r#"
        -module(test).
        -export([f/0]).
        -record(person, {name, age}).
        f() -> #person{name = "Alice", bogus = 42}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = undefined_record_field_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 error, got: {:?}", errs);
    assert!(errs[0].message.contains("bogus"), "should mention 'bogus': {:?}", errs[0]);
    assert!(errs[0].message.contains("person"), "should mention 'person': {:?}", errs[0]);
    assert!(errs[0].hint.as_ref().unwrap().contains("name"), "hint should list available fields: {:?}", errs[0]);
}

#[test]
fn undefined_record_field_in_update() {
    let src = r#"
        -module(test).
        -export([f/1]).
        -record(person, {name, age}).
        f(R) -> R#person{oops = 99}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = undefined_record_field_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 error, got: {:?}", errs);
    assert!(errs[0].message.contains("oops"), "should mention 'oops': {:?}", errs[0]);
}

#[test]
fn undefined_record_field_in_access() {
    let src = r#"
        -module(test).
        -export([f/1]).
        -record(person, {name, age}).
        f(R) -> R#person.missing.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = undefined_record_field_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 error, got: {:?}", errs);
    assert!(errs[0].message.contains("missing"), "should mention 'missing': {:?}", errs[0]);
}

#[test]
fn undefined_record_field_in_index() {
    let src = r#"
        -module(test).
        -export([f/0]).
        -record(person, {name, age}).
        f() -> #person.nope.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = undefined_record_field_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 error, got: {:?}", errs);
    assert!(errs[0].message.contains("nope"), "should mention 'nope': {:?}", errs[0]);
}

#[test]
fn undefined_record_field_in_pattern() {
    let src = r#"
        -module(test).
        -export([f/1]).
        -record(person, {name, age}).
        f(#person{wrong = X}) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = undefined_record_field_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 error, got: {:?}", errs);
    assert!(errs[0].message.contains("wrong"), "should mention 'wrong': {:?}", errs[0]);
}

#[test]
fn undefined_record_field_multiple_errors() {
    let src = r#"
        -module(test).
        -export([f/0]).
        -record(person, {name, age}).
        f() -> #person{bad1 = 1, bad2 = 2}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = undefined_record_field_errors(&ws);
    assert_eq!(errs.len(), 2, "expected 2 errors, got: {:?}", errs);
}

#[test]
fn undefined_record_field_skips_undefined_record() {
    // If the record itself is undefined, we should not emit field errors
    // (the undefined record error is reported during IR lowering).
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> #nonexistent{field = 1}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    assert!(
        undefined_record_field_errors(&ws).is_empty(),
        "should not report field errors for undefined records, got: {:?}",
        ws
    );
}

#[test]
fn undefined_record_field_severity_is_error() {
    let src = r#"
        -module(test).
        -export([f/0]).
        -record(person, {name, age}).
        f() -> #person{bogus = 1}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = undefined_record_field_errors(&ws);
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].severity, nera_syntax::error::Severity::Error);
}

#[test]
fn undefined_record_field_in_guard() {
    let src = r#"
        -module(test).
        -export([f/1]).
        -record(person, {name, age}).
        f(R) when R#person.bogus =:= ok -> R.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = undefined_record_field_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 error in guard, got: {:?}", errs);
    assert!(errs[0].message.contains("bogus"), "should mention 'bogus': {:?}", errs[0]);
}

#[test]
fn undefined_record_field_in_case_pattern() {
    let src = r#"
        -module(test).
        -export([f/1]).
        -record(person, {name, age}).
        f(X) ->
            case X of
                #person{bad = V} -> V
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = undefined_record_field_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 error in case pattern, got: {:?}", errs);
    assert!(errs[0].message.contains("bad"), "should mention 'bad': {:?}", errs[0]);
}

#[test]
fn undefined_record_field_in_fun_body() {
    let src = r#"
        -module(test).
        -export([f/0]).
        -record(person, {name, age}).
        f() ->
            F = fun(R) -> R#person.nope end,
            F.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = undefined_record_field_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 error in fun body, got: {:?}", errs);
}

#[test]
fn undefined_record_field_in_list_comprehension() {
    let src = r#"
        -module(test).
        -export([f/1]).
        -record(person, {name, age}).
        f(People) ->
            [P#person.zzz || P <- People].
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let errs = undefined_record_field_errors(&ws);
    assert_eq!(errs.len(), 1, "expected 1 error in list comprehension, got: {:?}", errs);
}

// ── Non-tail-recursion tests ────────────────────────────────

fn non_tail_warnings(ws: &[Diagnostic]) -> Vec<&Diagnostic> {
    ws.iter().filter(|w| w.code.as_deref() == Some("non_tail_recursion")).collect()
}

#[test]
fn non_tail_recursion_in_binop() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(0) -> 1;
        f(N) -> N * f(N - 1).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert_eq!(ntw.len(), 1, "expected 1 non-tail warning, got: {:?}", warning_messages(&ws));
    assert!(ntw[0].message.contains("f/1"));
}

#[test]
fn tail_recursion_no_warning() {
    let src = r#"
        -module(test).
        -export([f/2]).
        f(0, Acc) -> Acc;
        f(N, Acc) -> f(N - 1, Acc * N).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert!(ntw.is_empty(), "expected no non-tail warnings, got: {:?}", warning_messages(&ws));
}

#[test]
fn non_tail_recursion_in_cons() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f([]) -> [];
        f([H|T]) -> [H | f(T)].
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert_eq!(ntw.len(), 1, "expected 1 non-tail warning for cons, got: {:?}", warning_messages(&ws));
}

#[test]
fn mixed_tail_and_non_tail() {
    // Only the non-tail call should warn
    let src = r#"
        -module(test).
        -export([f/1]).
        f(0) -> 0;
        f(1) -> f(0);
        f(N) -> N + f(N - 1).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert_eq!(ntw.len(), 1, "expected 1 non-tail warning, got: {:?}", warning_messages(&ws));
}

#[test]
fn tail_call_in_case_body() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            case X of
                0 -> ok;
                N -> f(N - 1)
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert!(ntw.is_empty(), "case body tail call should not warn, got: {:?}", warning_messages(&ws));
}

#[test]
fn non_tail_in_try_body() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            try f(X - 1) of
                V -> V
            catch _:_ -> error
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert_eq!(ntw.len(), 1, "try body should be non-tail, got: {:?}", warning_messages(&ws));
}

#[test]
fn tail_call_in_try_of_clause() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            try some_op(X) of
                retry -> f(X);
                V -> V
            catch _:_ -> error
            end.
        some_op(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert!(ntw.is_empty(), "try-of clause body should be tail, got: {:?}", warning_messages(&ws));
}

#[test]
fn different_arity_not_warned() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) -> f(X, 0).
        f(0, Acc) -> Acc;
        f(N, Acc) -> f(N - 1, Acc + N).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert!(ntw.is_empty(), "different arity should not warn, got: {:?}", warning_messages(&ws));
}

// ── is_recursive tests ────────────────────────────────

#[test]
fn is_recursive_true() {
    let src = r#"
        -module(test).
        f(0) -> 1;
        f(N) -> N * f(N - 1).
    "#;
    let (module, _names) = parse_module(src);
    assert!(
        super::non_tail_recursion::is_recursive(&module.functions[0]),
        "f/1 should be detected as recursive"
    );
}

#[test]
fn is_recursive_false() {
    let src = r#"
        -module(test).
        f(X) -> X + 1.
    "#;
    let (module, _names) = parse_module(src);
    assert!(
        !super::non_tail_recursion::is_recursive(&module.functions[0]),
        "f/1 should not be detected as recursive"
    );
}

// ── Pragma tests ────────────────────────────────

fn parse_with_source(src: &str) -> (ast::Module, HashMap<AtomId, String>, String) {
    let mut parser = Parser::new(src);
    let result = parser.parse_module();
    assert!(result.errors.is_empty(), "parse errors: {:#?}", result.errors);
    let atom_names = parser.atom_names();
    (result.module, atom_names, src.to_string())
}

#[test]
fn pragma_suppresses_non_tail_recursion() {
    let src = r#"
-module(test).
-export([fib/1]).
%% @nera:nowarn(non_tail_recursion)
fib(0) -> 1;
fib(1) -> 1;
fib(N) -> fib(N - 1) + fib(N - 2).
    "#;
    let (module, names, source) = parse_with_source(src);
    let ws = analyze_module_with_source(&module, &names, None, Some(&source));
    let ntw = non_tail_warnings(&ws);
    assert!(ntw.is_empty(), "pragma should suppress non_tail_recursion, got: {:?}", warning_messages(&ws));
}

#[test]
fn pragma_on_wrong_function_does_not_suppress() {
    let src = r#"
-module(test).
-export([g/1, f/1]).
%% @nera:nowarn(non_tail_recursion)
g(X) -> X.
f(0) -> 1;
f(N) -> N * f(N - 1).
    "#;
    let (module, names, source) = parse_with_source(src);
    let ws = analyze_module_with_source(&module, &names, None, Some(&source));
    let ntw = non_tail_warnings(&ws);
    assert_eq!(ntw.len(), 1, "pragma on g should not suppress f's warning, got: {:?}", warning_messages(&ws));
}

#[test]
fn pragma_wrong_code_does_not_suppress() {
    let src = r#"
-module(test).
-export([f/1]).
%% @nera:nowarn(unused_variable)
f(0) -> 1;
f(N) -> N * f(N - 1).
    "#;
    let (module, names, source) = parse_with_source(src);
    let ws = analyze_module_with_source(&module, &names, None, Some(&source));
    let ntw = non_tail_warnings(&ws);
    assert_eq!(ntw.len(), 1, "wrong pragma code should not suppress, got: {:?}", warning_messages(&ws));
}

#[test]
fn tail_call_in_if_body() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(N) ->
            if
                N > 0 -> f(N - 1);
                true -> ok
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert!(ntw.is_empty(), "if body tail call should not warn, got: {:?}", warning_messages(&ws));
}

#[test]
fn tail_call_in_receive_body() {
    let src = r#"
        -module(test).
        -export([loop/0]).
        loop() ->
            receive
                {msg, _Data} -> loop();
                stop -> ok
            after 5000 ->
                loop()
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert!(ntw.is_empty(), "receive body tail call should not warn, got: {:?}", warning_messages(&ws));
}

#[test]
fn tail_call_in_try_catch_clause() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            try do_something(X)
            catch
                error:badarg -> f(X + 1);
                _:_ -> error
            end.
        do_something(X) -> X.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert!(ntw.is_empty(), "try-catch clause body should be tail, got: {:?}", warning_messages(&ws));
}

#[test]
fn non_tail_recursion_in_tuple() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(0) -> {ok, 0};
        f(N) -> {ok, f(N - 1)}.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert_eq!(ntw.len(), 1, "tuple element should be non-tail, got: {:?}", warning_messages(&ws));
}

#[test]
fn multiple_non_tail_calls_warn_each() {
    let src = r#"
        -module(test).
        -export([fib/1]).
        fib(0) -> 1;
        fib(1) -> 1;
        fib(N) -> fib(N - 1) + fib(N - 2).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert_eq!(ntw.len(), 2, "both fib calls should warn, got: {:?}", warning_messages(&ws));
}

#[test]
fn tail_call_via_match() {
    // `Result = f(X - 1)` in tail position — the match RHS inherits tail
    let src = r#"
        -module(test).
        -export([f/1]).
        f(0) -> done;
        f(X) -> _Result = f(X - 1).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let ntw = non_tail_warnings(&ws);
    assert!(ntw.is_empty(), "match RHS in tail position should not warn, got: {:?}", warning_messages(&ws));
}

#[test]
fn pragma_multiple_codes() {
    let src = r#"
-module(test).
-export([f/1]).
%% @nera:nowarn(non_tail_recursion)
%% @nera:nowarn(unused_variable)
f(0) -> 1;
f(N) ->
    Unused = 42,
    N * f(N - 1).
    "#;
    let (module, names, source) = parse_with_source(src);
    let ws = analyze_module_with_source(&module, &names, None, Some(&source));
    let ntw = non_tail_warnings(&ws);
    assert!(ntw.is_empty(), "non_tail_recursion should be suppressed");
    let unused: Vec<_> = ws.iter().filter(|w| w.code.as_deref() == Some("unused_variable")).collect();
    assert!(unused.is_empty(), "unused_variable should also be suppressed");
}

// ── Format string validation tests ─────────────────────

fn format_warnings(ws: &[Diagnostic]) -> Vec<&Diagnostic> {
    ws.iter()
        .filter(|w| w.code.as_deref() == Some("format_string_mismatch"))
        .collect()
}

#[test]
fn format_string_mismatch_too_few_args() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> io:format("~p ~p", [x]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert_eq!(fmt.len(), 1, "expected 1 format mismatch, got: {:?}", warning_messages(&ws));
    assert!(fmt[0].message.contains("expects 2") && fmt[0].message.contains("1 provided"));
}

#[test]
fn format_string_mismatch_too_many_args() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> io:format("~p", [x, y]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert_eq!(fmt.len(), 1);
    assert!(fmt[0].message.contains("expects 1") && fmt[0].message.contains("2 provided"));
}

#[test]
fn format_string_correct_no_warning() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> io:format("~p ~p", [x, y]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert!(fmt.is_empty(), "expected no format warnings, got: {:?}", warning_messages(&ws));
}

#[test]
fn format_string_io_lib() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> io_lib:format("~p ~s", [x]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert_eq!(fmt.len(), 1);
}

#[test]
fn format_string_io_format_3_device() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(Dev) -> io:format(Dev, "~p", [x]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert!(fmt.is_empty(), "io:format/3 with correct args should not warn");
}

#[test]
fn format_string_io_format_3_mismatch() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(Dev) -> io:format(Dev, "~p ~p", [x]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert_eq!(fmt.len(), 1);
}

#[test]
fn format_string_newline_not_counted() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> io:format("~p~n", [x]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert!(fmt.is_empty());
}

#[test]
fn format_string_escaped_tilde_not_counted() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> io:format("~~: ~p", [x]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert!(fmt.is_empty());
}

#[test]
fn format_string_big_p_consumes_two() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> io:format("~P", [x, 10]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert!(fmt.is_empty(), "~P consumes 2 args, 2 provided — no warning");
}

#[test]
fn format_string_big_p_mismatch() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> io:format("~P", [x]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert_eq!(fmt.len(), 1, "~P expects 2 args but got 1");
}

#[test]
fn format_string_variable_format_skipped() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(Fmt) -> io:format(Fmt, [x]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert!(fmt.is_empty(), "variable format string should be skipped");
}

#[test]
fn format_string_variable_args_skipped() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(Args) -> io:format("~p", Args).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert!(fmt.is_empty(), "variable arg list should be skipped");
}

#[test]
fn format_string_no_args_newline_only() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> io:format("hello~n", []).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert!(fmt.is_empty());
}

#[test]
fn format_string_ignore_directive_consumes_one() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> io:format("~p~i~p", [x, y, z]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert!(fmt.is_empty(), "~i consumes 1 arg");
}

#[test]
fn format_string_nested_in_case() {
    let src = r#"
        -module(test).
        -export([f/1]).
        f(X) ->
            case X of
                ok -> io:format("~p ~p", [X]);
                _ -> ok
            end.
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert_eq!(fmt.len(), 1, "should detect mismatch inside case clause");
}

#[test]
fn format_string_fwrite_also_checked() {
    let src = r#"
        -module(test).
        -export([f/0]).
        f() -> io:fwrite("~p ~p", [x]).
    "#;
    let (module, names) = parse_module(src);
    let ws = analyze_module(&module, &names, None);
    let fmt = format_warnings(&ws);
    assert_eq!(fmt.len(), 1, "io:fwrite should be checked like io:format");
}

// ── Unused macro end-to-end tests ─────────────────────────────────

fn preprocess_and_check_macros(src: &str) -> Vec<Diagnostic> {
    use std::path::PathBuf;
    use nera_syntax::lexer::Lexer;
    use nera_syntax::preproc::Preprocessor;

    let tokens = Lexer::lex_all(src).unwrap();
    let path = PathBuf::from("test.erl");
    let mut pp = Preprocessor::new(src, &path);
    let _ = pp.process(tokens).unwrap();
    let (_, _, macro_locs, used_macros) = pp.take_all();

    let mut warnings = Vec::new();
    super::unused_macros::check_unused_macros(&macro_locs, &used_macros, &mut warnings);
    warnings
}

fn unused_macro_warnings(warnings: &[Diagnostic]) -> Vec<&Diagnostic> {
    warnings.iter().filter(|w| w.code.as_deref() == Some("unused_macro")).collect()
}

#[test]
fn unused_macro_e2e_warns() {
    let src = "-module(test).\n-define(FOO, bar).\nf() -> ok.";
    let ws = preprocess_and_check_macros(src);
    let uw = unused_macro_warnings(&ws);
    assert_eq!(uw.len(), 1, "expected 1 unused_macro warning, got: {:?}", warning_messages(&ws));
    assert!(uw[0].message.contains("'FOO'"));
}

#[test]
fn used_macro_e2e_no_warning() {
    let src = "-module(test).\n-define(FOO, bar).\nf() -> ?FOO.";
    let ws = preprocess_and_check_macros(src);
    assert!(
        unused_macro_warnings(&ws).is_empty(),
        "macro used via ?FOO should not warn, got: {:?}", warning_messages(&ws)
    );
}

#[test]
fn macro_ifdef_reference_e2e_no_warning() {
    let src = "-module(test).\n-define(DEBUG).\n-ifdef(DEBUG).\nf() -> debug.\n-endif.";
    let ws = preprocess_and_check_macros(src);
    assert!(
        unused_macro_warnings(&ws).is_empty(),
        "-ifdef reference should suppress warning, got: {:?}", warning_messages(&ws)
    );
}

#[test]
fn macro_ifndef_reference_e2e_no_warning() {
    let src = "-module(test).\n-define(PROD).\n-ifndef(PROD).\nf() -> debug.\n-else.\nf() -> prod.\n-endif.";
    let ws = preprocess_and_check_macros(src);
    assert!(
        unused_macro_warnings(&ws).is_empty(),
        "-ifndef reference should suppress warning, got: {:?}", warning_messages(&ws)
    );
}

#[test]
fn multiple_macros_e2e_partial_use() {
    let src = "-module(test).\n-define(USED, 1).\n-define(UNUSED, 2).\nf() -> ?USED.";
    let ws = preprocess_and_check_macros(src);
    let uw = unused_macro_warnings(&ws);
    assert_eq!(uw.len(), 1, "expected 1 unused warning, got: {:?}", warning_messages(&ws));
    assert!(uw[0].message.contains("'UNUSED'"));
}

#[test]
fn function_macro_e2e_used_no_warning() {
    let src = "-module(test).\n-define(ADD(X, Y), X + Y).\nf() -> ?ADD(1, 2).";
    let ws = preprocess_and_check_macros(src);
    assert!(
        unused_macro_warnings(&ws).is_empty(),
        "function macro used via ?ADD should not warn, got: {:?}", warning_messages(&ws)
    );
}

#[test]
fn function_macro_e2e_unused_warns() {
    let src = "-module(test).\n-define(ADD(X, Y), X + Y).\nf() -> ok.";
    let ws = preprocess_and_check_macros(src);
    let uw = unused_macro_warnings(&ws);
    assert_eq!(uw.len(), 1);
    assert!(uw[0].message.contains("'ADD'"));
}

#[test]
fn imported_functions_are_defined_only_at_their_declared_arity() {
    let (module, names) = parse_module("-module(p). -import(lists, [reverse/1]). -export([test/0]). test() -> {reverse([]), fun reverse/1, reverse([], [])}.");
    let ws = analyze_module(&module, &names, None);
    let undefined: Vec<_> = ws.iter().filter(|w| w.code.as_deref() == Some("undefined_function")).collect();
    assert_eq!(undefined.len(), 1);
    assert!(undefined[0].message.contains("reverse/2"));
}

#[test]
fn imported_bif_name_is_not_allowed_in_guards() {
    let (module, names) = parse_module("-module(p). -import(other, [float/1]). -export([test/1]). test(X) when float(X) > 0 -> ok.");
    let ws = analyze_module(&module, &names, None);
    assert!(ws.iter().any(|w| w.code.as_deref() == Some("illegal_guard_local_call")), "{ws:?}");
}

#[test]
fn comprehension_patterns_receive_binary_segment_checks() {
    // NERA-73 F3: an unsized <= field previously bypassed lint and looped.
    // Both comprehension forms and <- patterns share the segment validator.
    for comprehension in ["[X || PAT <= B]", "<< X || PAT <= B >>"] {
        for (pattern, message) in [
            ("<<X/binary>>", "binary fields without size are not allowed"),
            ("<<X/bits>>", "binary fields without size are not allowed"),
            ("<<_:8,X/binary>>", "binary fields without size are not allowed"),
            ("<<X:8/float>>", "invalid size 8 for float segment"),
            ("<<X:8/utf8>>", "size not allowed for utf8 segments"),
            ("<<X:8/utf16>>", "size not allowed for utf16 segments"),
            ("<<X:8/utf32>>", "size not allowed for utf32 segments"),
        ] {
            let body = comprehension.replace("PAT", pattern);
            let source = format!("-module(p). f(B) -> {body}.");
            let (module, names) = parse_module(&source);
            let diagnostics = analyze_module(&module, &names, None);
            assert!(diagnostics.iter().any(|d|
                d.severity == nera_syntax::error::Severity::Error
                && d.code.as_deref() == Some("invalid_binary_segment")
                && d.message.contains(message)), "{body}: {diagnostics:?}");
        }
    }
    for (pattern, invalid) in [("<<X/binary>>", false), ("<<X:8/float>>", true)] {
        let (module, names) = parse_module(&format!("-module(p). f(L) -> [X || {pattern} <- L]."));
        let diagnostics = analyze_module(&module, &names, None);
        assert_eq!(diagnostics.iter().any(|d|
            d.code.as_deref() == Some("invalid_binary_segment")), invalid);
    }
}

#[test]
fn binary_generator_lint_reports_each_unsized_field_once() {
    // NERA-100 F2: as in OTP, the generator rule covers the last field only.
    // An earlier unsized field already gets the "only allowed at the end"
    // error from the shared pattern check, so each field reports once.
    let at_end = "a binary segment without size is only allowed at the end of a binary pattern";
    let in_generator = "binary fields without size are not allowed in patterns of bit string generators";
    for (body, expected) in [
        ("[X || <<X/binary, _>> <= B]", vec![at_end]),
        ("[X || <<X:8, _/binary, Y/binary>> <= B]", vec![at_end, in_generator]),
        ("<< X || <<X/bits>> <= B >>", vec![in_generator]),
    ] {
        let (module, names) = parse_module(&format!("-module(p). f(B) -> {body}."));
        let messages: Vec<String> = analyze_module(&module, &names, None).into_iter()
            .filter(|d| d.code.as_deref() == Some("invalid_binary_segment"))
            .map(|d| d.message).collect();
        assert_eq!(messages, expected, "{body}");
    }
}

#[test]
fn binary_segment_checks_walk_call_targets() {
    // NERA-100 F1: the walker skipped callee expressions, so a fun called in
    // place hid its segments from every check.
    for body in [
        "(fun() -> [X || <<X/binary>> <= B] end)()",
        "(fun() -> << X || <<X/bits>> <= B >> end)()",
        "(fun() -> <<X:8/float>> = B end)()",
        "(fun() -> <<_:8/utf8>> = B end)()",
    ] {
        let (module, names) = parse_module(&format!("-module(p). f(B) -> {body}."));
        let diagnostics = analyze_module(&module, &names, None);
        assert!(diagnostics.iter().any(|d|
            d.severity == nera_syntax::error::Severity::Error
            && d.code.as_deref() == Some("invalid_binary_segment")), "{body}: {diagnostics:?}");
    }
}

#[test]
fn binary_segment_checks_walk_guards_sizes_keys_and_record_defaults() {
    // NERA-103 F1: these positions never reached the walker, and lowering
    // discards errors from guards, pattern sizes and map keys. OTP rejects
    // the record default even when the record is unused or overridden.
    let bad = "[Y || <<Y/binary>> <= <<>>]";
    for (source, position) in [
        (format!("-module(p). -record(r, {{f = {bad}}})."), "unused record default"),
        (format!("-module(p). -record(r, {{f = {bad}}}). f() -> #r{{f = 1}}."), "overridden record default"),
        (format!("-module(p). f(B) when {bad} =:= B -> ok."), "function guard"),
        (format!("-module(p). f(B) -> case B of _ when {bad} =:= [] -> ok end."), "case guard"),
        (format!("-module(p). f(B) -> if {bad} =:= B -> ok end."), "if guard"),
        (format!("-module(p). f(B) -> receive _ when {bad} =:= B -> ok end."), "receive guard"),
        (format!("-module(p). f(B) -> try B of _ when {bad} =:= [] -> ok catch _:_ when {bad} =:= [] -> ok end."), "try guards"),
        (format!("-module(p). f() -> fun(B) when {bad} =:= B -> ok end."), "fun guard"),
        (format!("-module(p). f(B) -> maybe ok ?= B else _ when {bad} =:= [] -> ok end."), "maybe else guard"),
        (format!("-module(p). f(<<N:({bad}), X:N>>) -> X."), "binary pattern size"),
        (format!("-module(p). f(#{{{bad} := V}}) -> V."), "map pattern key"),
        (format!("-module(p). f(\"p\" ++ [<<X:({bad})>>]) -> X."), "string concat pattern"),
        (format!("-module(p). f(R) -> R#r{{f = {bad}}}."), "record update"),
    ] {
        let (module, names) = parse_module(&source);
        let diagnostics = analyze_module(&module, &names, None);
        assert!(diagnostics.iter().any(|d|
            d.severity == nera_syntax::error::Severity::Error
            && d.code.as_deref() == Some("invalid_binary_segment")
            && d.message.contains("bit string generators")), "{position}: {diagnostics:?}");
    }
}

#[test]
fn float_segment_size_is_scaled_by_unit() {
    // NERA-103 F2: OTP's bit_size_check accepts a float field whose size
    // times unit is 16, 32 or 64 bits, so `<<X:1/float-unit:64>>` is valid.
    let errors = |body: &str| -> Vec<String> {
        let (module, names) = parse_module(&format!("-module(p). f(B) -> {body}."));
        analyze_module(&module, &names, None).into_iter()
            .filter(|d| d.code.as_deref() == Some("invalid_binary_segment"))
            .map(|d| d.message).collect()
    };
    for segment in ["X:1/float-unit:64", "X:2/float-unit:32", "X:4/float-unit:16", "X:8/float-unit:8"] {
        for body in [
            format!("<<{segment}>>"),
            format!("<<{segment}>> = B"),
            format!("[X || <<{segment}>> <= B]"),
            format!("<< <<X:64/float>> || <<{segment}>> <= B >>"),
            format!("[X || <<{segment}>> <- B]"),
        ] {
            assert_eq!(errors(&body), Vec::<String>::new(), "{body}");
        }
    }
    assert_eq!(errors("<<X:2/float-unit:64>> = B"),
        vec!["invalid size 128 for float segment (must be 16, 32, or 64)"]);
    assert_eq!(errors("[X || <<X:3/float-unit:8>> <= B]"),
        vec!["invalid size 24 for float segment (must be 16, 32, or 64)"]);
    assert_eq!(errors("<<X:1/float>> = B"),
        vec!["invalid size 1 for float segment (must be 16, 32, or 64)"]);
}
