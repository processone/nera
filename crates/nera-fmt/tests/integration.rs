use nera_fmt::config::FormatConfig;

fn default_config() -> FormatConfig {
    FormatConfig::default()
}

fn format(source: &str) -> String {
    nera_fmt::format_source(source, &default_config()).unwrap()
}

// ── Idempotence tests ───────────────────────────────────────────────

#[test]
fn idempotent_simple_module() {
    let src = "-module(test).\n-export([hello/0]).\nhello() -> world.\n";
    let first = format(src);
    let second = format(&first);
    assert_eq!(first, second, "formatting is not idempotent");
}

#[test]
fn idempotent_case() {
    let src = "-module(test).\n-export([f/1]).\nf(X) -> case X of 1 -> one; 2 -> two end.\n";
    let first = format(src);
    let second = format(&first);
    assert_eq!(first, second, "case formatting is not idempotent");
}

#[test]
fn idempotent_multi_clause() {
    let src = "-module(test).\n-export([f/1]).\nf(0) -> zero;\nf(N) -> N.\n";
    let first = format(src);
    let second = format(&first);
    assert_eq!(first, second, "multi-clause formatting is not idempotent");
}

// ── Fixture tests ───────────────────────────────────────────────────

#[test]
fn format_fixture_input() {
    let src = include_str!("fixtures/input.erl");
    let result = format(src);
    // Should produce valid output
    assert!(result.contains("-module(example)."));
    assert!(result.contains("hello()"));
    assert!(result.contains("factorial"));
    // Should be idempotent
    let second = format(&result);
    assert_eq!(result, second, "fixture formatting is not idempotent");
}

#[test]
fn format_fixture_preprocessor() {
    let src = include_str!("fixtures/preprocessor.erl");
    let result = format(src);
    // Simple defines
    assert!(result.contains("-define(DEBUG)."), "missing -define(DEBUG).");
    assert!(result.contains("-define(TIMEOUT, 5000)."), "missing TIMEOUT define");
    assert!(result.contains("-define(APP_NAME, \"myapp\")."), "missing APP_NAME define");
    assert!(result.contains("-define(ADD(X, Y), X + Y)."), "missing ADD define");
    // Conditional compilation
    assert!(result.contains("-ifdef(DEBUG)."), "missing -ifdef(DEBUG).");
    assert!(result.contains("-define(LOG(X), io:format(\"~p~n\", [X]))."), "missing LOG define");
    assert!(result.contains("-else."), "missing -else.");
    assert!(result.contains("-endif."), "missing -endif.");
    // Include directives
    assert!(result.contains("-include(\"header.hrl\")."), "missing -include");
    assert!(result.contains("-include_lib(\"kernel/include/file.hrl\")."), "missing -include_lib");
    // Undef
    assert!(result.contains("-undef(DEBUG)."), "missing -undef");
    // Tuple body define
    assert!(result.contains("-define(PAIR(A, B), {A, B})."), "missing PAIR define");
    // Whitespace normalized: -ifdef(  TEST  ) -> -ifdef(TEST)
    assert!(result.contains("-ifdef(TEST)."), "whitespace not normalized in -ifdef(TEST)");
    // Idempotence
    let second = format(&result);
    assert_eq!(result, second, "preprocessor formatting is not idempotent");
}

#[test]
fn format_fixture_records_maps() {
    let src = include_str!("fixtures/records_maps.erl");
    let result = format(src);
    assert!(result.contains("#person{"));
    assert!(result.contains("#{"));
    // Idempotent
    let second = format(&result);
    assert_eq!(result, second);
}

// ── check_format tests ─────────────────────────────────────────────

#[test]
fn check_format_already_formatted() {
    let src = format("-module(test).\n-export([f/0]).\nf() -> ok.\n");
    assert!(nera_fmt::check_format(&src, &default_config()).unwrap());
}

// ── Form splitting tests ────────────────────────────────────────────

#[test]
fn preserves_trailing_newline() {
    let src = "-module(test).\nhello() -> ok.\n";
    let result = format(src);
    assert!(result.ends_with('\n'));
}

#[test]
fn handles_empty_source() {
    let result = format("");
    assert!(result.is_empty() || result == "\n");
}

// ── Per-construct tests ─────────────────────────────────────────────

#[test]
fn format_if_expression() {
    let src = "-module(test).\n-export([f/1]).\nf(X) -> if X > 0 -> positive; true -> non_positive end.\n";
    let result = format(src);
    assert!(result.contains("if"));
    assert!(result.contains("end"));
}

#[test]
fn format_receive_with_after() {
    let src = "-module(test).\n-export([f/0]).\nf() -> receive {msg, V} -> V after 5000 -> timeout end.\n";
    let result = format(src);
    assert!(result.contains("receive"));
    assert!(result.contains("after"));
    assert!(result.contains("end"));
}

#[test]
fn format_binary_expression() {
    let src = "-module(test).\n-export([f/0]).\nf() -> <<1, 2, 3>>.\n";
    let result = format(src);
    assert!(result.contains("<<"));
    assert!(result.contains(">>"));
}

#[test]
fn format_anonymous_fun() {
    let src = "-module(test).\n-export([f/0]).\nf() -> fun(X) -> X + 1 end.\n";
    let result = format(src);
    assert!(result.contains("fun"));
    assert!(result.contains("end"));
}

#[test]
fn format_list_comprehension() {
    let src = "-module(test).\n-export([f/1]).\nf(L) -> [X * 2 || X <- L, X > 0].\n";
    let result = format(src);
    assert!(result.contains("||"));
    assert!(result.contains("<-"));
}

#[test]
fn format_send_operator() {
    let src = "-module(test).\n-export([f/1]).\nf(Pid) -> Pid ! hello.\n";
    let result = format(src);
    assert!(result.contains("!"));
}

#[test]
fn format_map_create_and_update() {
    let src = "-module(test).\n-export([f/0]).\nf() -> M = #{a => 1}, M#{a := 2}.\n";
    let result = format(src);
    assert!(result.contains("#{"));
    assert!(result.contains("=>"));
}

#[test]
fn format_begin_end_block() {
    let src = "-module(test).\n-export([f/0]).\nf() -> begin 1 + 2 end.\n";
    let result = format(src);
    assert!(result.contains("begin"));
    assert!(result.contains("end"));
}

#[test]
fn format_try_catch_after() {
    let src = "-module(test).\n-export([f/0]).\nf() -> try 1 catch error:E -> E after cleanup end.\n";
    let result = format(src);
    assert!(result.contains("try"));
    assert!(result.contains("catch"));
    assert!(result.contains("after"));
}

// ── Export formatting tests ─────────────────────────────────────────

#[test]
fn format_export_short_stays_flat() {
    let src = "-module(test).\n-export([f/0]).\nf() -> ok.\n";
    let result = format(src);
    // Short export should stay on one line with no extra space after [
    assert!(result.contains("-export([f/0])."), "expected flat export, got:\n{}", result);
}

#[test]
fn format_export_multiline_breaks_correctly() {
    // Many exports to exceed max_width (100)
    let src = "-module(test).\n\
               -export([aaa_long_name/1, bbb_long_name/2, ccc_long_name/3, ddd_long_name/0, eee_long_name/1, fff_long_name/2]).\n\
               aaa_long_name(_) -> ok.\nbbb_long_name(_, _) -> ok.\nccc_long_name(_, _, _) -> ok.\n\
               ddd_long_name() -> ok.\neee_long_name(_) -> ok.\nfff_long_name(_, _) -> ok.\n";
    let result = format(src);
    // Entries should be indented by 4 spaces
    assert!(result.contains("\n    aaa_long_name/1"), "entries should be at 4-space indent:\n{}", result);
    // Closing ]). should be on its own line at column 0
    assert!(result.contains("\n])."), "closing ]). should be on its own line at col 0:\n{}", result);
    // Idempotent
    let second = format(&result);
    assert_eq!(result, second, "multi-line export formatting is not idempotent");
}

#[test]
fn format_export_no_space_after_bracket() {
    let src = "-module(test).\n-export([hello/0, world/1]).\nhello() -> ok.\nworld(_) -> ok.\n";
    let result = format(src);
    // Should not have a space between [ and first entry
    assert!(!result.contains("-export([ "), "should not have space after [:\n{}", result);
}

// ── Fill-mode (compact) formatting tests ────────────────────────────

#[test]
fn fill_short_tuple_stays_flat() {
    // Short tuple that fits on one line should stay flat.
    let src = "-module(t).\n-export([f/0]).\nf() -> {ok, 1, 2, 3}.\n";
    let result = format(src);
    assert!(
        result.contains("{ok, 1, 2, 3}"),
        "short tuple should stay flat, got:\n{}",
        result
    );
}

#[test]
fn fill_short_list_stays_flat() {
    let src = "-module(t).\n-export([f/0]).\nf() -> [1, 2, 3, 4, 5].\n";
    let result = format(src);
    assert!(
        result.contains("[1, 2, 3, 4, 5]"),
        "short list should stay flat, got:\n{}",
        result
    );
}

#[test]
fn long_tuple_hangs_one_element_per_line() {
    // A tuple that doesn't fit on one line breaks with hanging brackets:
    // `{` on its own line, each element on its own line indented, `}` on its
    // own line. This is the erlfmt / prettier "consistent break" style.
    let src = "-module(t).\n-export([f/0]).\n\
               f() -> {element_aaa, element_bbb, element_ccc, element_ddd, element_eee, element_fff, element_ggg, element_hhh}.\n";
    let result = format(src);
    // One element per line.
    let element_lines: Vec<&str> = result
        .lines()
        .filter(|l| l.contains("element_"))
        .collect();
    assert_eq!(
        element_lines.len(),
        8,
        "expected 8 element lines (one per element), got {}:\n{}",
        element_lines.len(),
        result
    );
    // Hanging opening brace on its own line.
    assert!(
        result.lines().any(|l| l.trim() == "{"),
        "expected '{{' on its own line, got:\n{}",
        result
    );
    // Hanging closing brace.
    assert!(
        result.lines().any(|l| l.trim() == "}."),
        "expected '}}.' on its own line, got:\n{}",
        result
    );
    // No element shares a line with the opening brace.
    assert!(
        !result.contains("{element_aaa"),
        "first element should NOT be on the brace line (hanging-bracket style), got:\n{}",
        result
    );
}

#[test]
fn fill_first_element_stays_with_bracket() {
    // When a list breaks, the first element should stay on the line with '['.
    let src = "-module(t).\n-export([f/0]).\n\
               f() -> [aaaaa, bbbbb, ccccc, ddddd, eeeee, fffff, ggggg, hhhhh, iiiii, jjjjj, kkkkk, lllll].\n";
    let result = format(src);
    assert!(
        result.contains("[aaaaa"),
        "first element should stay with opening bracket, got:\n{}",
        result
    );
}

#[test]
fn fill_supervisor_child_spec_idempotent() {
    // Supervisor-style nested tuple that forces breaks.
    let src = "-module(sup).\n-export([init/1]).\n\
               init(_) -> {ok, {{one_for_one, 1, 10}, [{worker_one, {worker_one, start_link, []}, permanent, 5000, worker, [worker_one]}]}}.\n";
    let first = format(src);
    let second = format(&first);
    assert_eq!(first, second, "supervisor formatting not idempotent:\n{}", first);
    // Result should be compact (no lines containing only whitespace + single atom)
    let lonely_atom_lines = first
        .lines()
        .filter(|l| {
            let trimmed = l.trim_end_matches(',').trim();
            !trimmed.is_empty()
                && trimmed.chars().all(|c| c.is_alphanumeric() || c == '_')
                && trimmed.len() < 20
        })
        .count();
    assert!(
        lonely_atom_lines < 5,
        "too many single-atom lines (output is too exploded):\n{}",
        first
    );
}

#[test]
fn fill_idempotent_nested_tuples() {
    let src = "-module(t).\n-export([f/0]).\n\
               f() -> {a, b, {c, d, {e, f, {g, h, {i, j, k, l, m, n, o, p, q, r, s, t, u}}}}}.\n";
    let first = format(src);
    let second = format(&first);
    assert_eq!(first, second, "nested tuple formatting not idempotent:\n{}", first);
}

#[test]
fn fill_idempotent_long_list() {
    let src = "-module(t).\n-export([f/0]).\n\
               f() -> [apple, banana, cherry, date, elderberry, fig, grape, honeydew, kiwi, lemon, mango, nectarine, orange, peach, quince, raspberry, strawberry, tangerine].\n";
    let first = format(src);
    let second = format(&first);
    assert_eq!(first, second, "long list formatting not idempotent:\n{}", first);
}

#[test]
fn fill_long_function_call_args() {
    let src = "-module(t).\n-export([f/0]).\n\
               f() -> some_module:some_function(argument_one, argument_two, argument_three, argument_four, argument_five).\n";
    let result = format(src);
    // Idempotence
    let second = format(&result);
    assert_eq!(result, second, "function call formatting not idempotent:\n{}", result);
}

#[test]
fn fill_pattern_tuple_wraps() {
    // Tuple in pattern position should also use fill.
    let src = "-module(t).\n-export([f/1]).\n\
               f({aaa_long, bbb_long, ccc_long, ddd_long, eee_long, fff_long, ggg_long}) -> ok.\n";
    let result = format(src);
    let second = format(&result);
    assert_eq!(result, second, "pattern tuple formatting not idempotent:\n{}", result);
}

// ── Function head breaking tests ──────────────────────────────────

#[test]
fn function_head_stays_flat_when_params_fit() {
    let src = "-module(t).\n-export([f/3]).\nf(A, B, C) -> ok.\n";
    let result = format(src);
    assert!(
        result.contains("f(A, B, C) -> ok."),
        "short params should stay flat, got:\n{}",
        result
    );
}

#[test]
fn function_head_breaks_params_erlfmt_style() {
    // When params don't fit on one line, erlfmt moves first param down,
    // puts each on its own line, and closes `)` on its own line.
    let src = "-module(t).\n-export([f/5]).\n\
               f(VeryVeryLongParamName1, VeryVeryLongParamName2, VeryVeryLongParamName3, VeryVeryLongParamName4, VeryVeryLongParamName5) -> ok.\n";
    let result = format(src);
    // First param on its own line after `(`
    assert!(
        result.contains("f(\n    VeryVeryLongParamName1,"),
        "first param should break to next line, got:\n{}",
        result
    );
    // Closing `)` on its own line
    assert!(
        result.contains("\n) -> ok."),
        "closing paren should be on its own line with arrow, got:\n{}",
        result
    );
    // Idempotence
    let second = format(&result);
    assert_eq!(result, second, "not idempotent:\n{}", result);
}

#[test]
fn function_head_breaks_params_with_guard() {
    let src = "-module(t).\n-export([g/5]).\n\
               g(VeryVeryLongParamName1, VeryVeryLongParamName2, VeryVeryLongParamName3, VeryVeryLongParamName4, VeryVeryLongParamName5) when VeryVeryLongParamName1 > 0 -> ok.\n";
    let result = format(src);
    // Params should break
    assert!(
        result.contains("g(\n    VeryVeryLongParamName1,"),
        "params should break, got:\n{}",
        result
    );
    // Guard follows `)` (when_guard decides independently)
    assert!(
        result.contains(") when VeryVeryLongParamName1 > 0 -> ok."),
        "guard should follow closing paren, got:\n{}",
        result
    );
    // Idempotence
    let second = format(&result);
    assert_eq!(result, second, "not idempotent:\n{}", result);
}

#[test]
fn fun_clause_breaks_params_erlfmt_style() {
    // Anonymous function clauses should also break params with hanging brackets.
    let src = "-module(t).\n-export([f/0]).\n\
               f() -> fun(VeryVeryLongParamName1, VeryVeryLongParamName2, VeryVeryLongParamName3, VeryVeryLongParamName4) -> VeryVeryLongParamName1 end.\n";
    let result = format(src);
    // Params should break inside fun
    assert!(
        result.contains("fun (\n"),
        "fun clause params should break with hanging bracket, got:\n{}",
        result
    );
    // Closing `)` on its own line at fun's indent level
    assert!(
        result.contains("\n    ) ->"),
        "closing paren should be on its own line, got:\n{}",
        result
    );
    // Idempotence
    let second = format(&result);
    assert_eq!(result, second, "not idempotent:\n{}", result);
}

// ── Term file (.app) formatting tests ──────────────────────────────

fn format_term(source: &str) -> String {
    nera_fmt::format_term_source(source, &default_config()).unwrap()
}

#[test]
fn fill_app_file_simple() {
    let src = "{application, myapp, [{description, \"An app\"}, {vsn, \"1.0.0\"}]}.\n";
    let result = format_term(src);
    let second = format_term(&result);
    assert_eq!(result, second, "app file formatting not idempotent:\n{}", result);
}

#[test]
fn fill_app_file_realistic() {
    let src = "{application, myapp,\n\
              [{description, \"A realistic Erlang application\"},\n\
               {vsn, \"1.0.0\"},\n\
               {registered, [myapp_server, myapp_client]},\n\
               {applications, [kernel, stdlib, crypto, ssl, inets, public_key]},\n\
               {mod, {myapp_app, []}},\n\
               {modules, [myapp_app, myapp_sup, myapp_server, myapp_client, myapp_utils]}]}.\n";
    let result = format_term(src);
    let second = format_term(&result);
    assert_eq!(result, second, "realistic .app formatting not idempotent:\n{}", result);
    // Result should contain expected content
    assert!(result.contains("application"));
    assert!(result.contains("myapp"));
    assert!(result.contains("kernel"));
}

#[test]
fn preserve_blank_line_between_comments() {
    // Blank line between section header comments should be preserved
    let src = "\
-module(test).
-export([init/0]).

%%===================================================
%% API functions
%%===================================================

%% Initialize the system.

init() -> ok.
";
    let result = format(src);
    // Blank line between section header and function comment must be preserved
    assert!(
        result.contains("%%===================================================\n\n%% Initialize"),
        "blank line between section header and function comment was removed:\n{}",
        result
    );
    // Blank line between last section comment and the function must be preserved
    assert!(
        result.contains("%% Initialize the system.\n\ninit()"),
        "blank line between comment and function was removed:\n{}",
        result
    );
    // Idempotent
    let second = format(&result);
    assert_eq!(result, second, "comment blank line preservation is not idempotent");
}

// ── Spec / type / callback / opaque formatting ─────────────────────

#[test]
fn spec_simple_stays_flat() {
    let src = "-module(t).\n-spec test() -> ok.\n";
    let result = format(src);
    assert!(result.contains("-spec test() -> ok.\n"));
}

#[test]
fn spec_with_params_stays_flat() {
    let src = "-module(t).\n-spec foo(atom(), integer()) -> ok.\n";
    let result = format(src);
    assert!(result.contains("-spec foo(atom(), integer()) -> ok.\n"));
}

#[test]
fn spec_long_breaks_at_arrow() {
    let src = "-module(t).\n-spec very_long_function_name(some_long_type(), another_really_long_type()) -> {ok, some_result()} | {error, reason()}.\n";
    let result = format(src);
    assert!(
        result.contains("-spec very_long_function_name(some_long_type(), another_really_long_type()) ->\n"),
        "spec should break at arrow:\n{}",
        result
    );
}

#[test]
fn spec_when_constraint() {
    let src = "-module(t).\n-spec id(X) -> X when X :: term().\n";
    let result = format(src);
    assert!(result.contains("-spec id(X) -> X when X :: term().\n"));
}

#[test]
fn spec_when_constraint_breaks() {
    let src = "-module(t).\n-spec lookup(Key, Map) -> {ok, Value} | error when Key :: term(), Map :: #{term() => term()}, Value :: term().\n";
    let result = format(src);
    assert!(
        result.contains("when\n"),
        "when constraints should break:\n{}",
        result
    );
    assert!(
        result.contains("    Key :: term(),\n"),
        "constraints should be indented:\n{}",
        result
    );
}

#[test]
fn spec_multi_clause_alignment() {
    let src = "-module(t).\n-spec convert(integer()) -> binary(); (binary()) -> integer().\n";
    let result = format(src);
    assert!(
        result.contains("-spec convert(integer()) -> binary();\n"),
        "multi-clause spec should have semicolon then newline:\n{}",
        result
    );
    assert!(
        result.contains("             (binary()) -> integer().\n"),
        "second clause should align under opening paren:\n{}",
        result
    );
}

#[test]
fn type_simple_flat() {
    let src = "-module(t).\n-type name() :: atom().\n";
    let result = format(src);
    assert!(result.contains("-type name() :: atom().\n"));
}

#[test]
fn type_union() {
    let src = "-module(t).\n-type result() :: {ok, term()} | {error, term()}.\n";
    let result = format(src);
    assert!(result.contains("-type result() :: {ok, term()} | {error, term()}.\n"));
}

#[test]
fn opaque_formatting() {
    let src = "-module(t).\n-opaque internal() :: {secret, term()}.\n";
    let result = format(src);
    assert!(result.contains("-opaque internal() :: {secret, term()}.\n"));
}

#[test]
fn callback_formatting() {
    let src = "-module(t).\n-callback init(Args :: term()) -> {ok, term()}.\n";
    let result = format(src);
    assert!(result.contains("-callback init(Args :: term()) -> {ok, term()}.\n"));
}

#[test]
fn type_with_range_formatting() {
    let src = "-module(t).\n-type byte_val() :: 0..255.\n";
    let result = format(src);
    assert!(result.contains("-type byte_val() :: 0..255.\n"));
}

#[test]
fn type_with_fun_type() {
    let src = "-module(t).\n-type handler() :: fun((term()) -> ok | error).\n";
    let result = format(src);
    assert!(result.contains("-type handler() :: fun((term()) -> ok | error).\n"));
}

#[test]
fn type_with_map_type() {
    let src = "-module(t).\n-type my_map() :: #{atom() => term()}.\n";
    let result = format(src);
    assert!(result.contains("-type my_map() :: #{atom() => term()}.\n"));
}

#[test]
fn type_with_record_type() {
    let src = "-module(t).\n-type state() :: #state{}.\n";
    let result = format(src);
    assert!(result.contains("-type state() :: #state{}.\n"));
}

#[test]
fn spec_idempotent() {
    let src = concat!(
        "-module(t).\n",
        "-spec test() -> ok.\n",
        "-spec foo(atom(), integer()) -> ok.\n",
        "-spec id(X) -> X when X :: term().\n",
        "-type name() :: atom().\n",
        "-type result() :: {ok, term()} | {error, term()}.\n",
        "-callback init(Args :: term()) -> {ok, term()}.\n",
        "-opaque internal() :: {secret, term()}.\n",
        "-type byte_val() :: 0..255.\n",
        "-type handler() :: fun((term()) -> ok | error).\n",
        "-spec convert(integer()) -> binary(); (binary()) -> integer().\n",
    );
    let first = format(src);
    let second = format(&first);
    assert_eq!(first, second, "spec formatting is not idempotent");
}

#[test]
fn spec_parenthesized_form() {
    let src = "-module(t).\n-spec(foo(atom()) -> ok).\n";
    let result = format(src);
    // Parenthesized form should be normalized to non-parenthesized
    assert!(
        result.contains("-spec foo(atom()) -> ok.\n"),
        "parenthesized spec should be normalized:\n{}",
        result
    );
}

#[test]
fn spec_annotated_params() {
    let src = "-module(t).\n-spec foo(Name :: atom(), Age :: integer()) -> ok.\n";
    let result = format(src);
    assert!(result.contains("-spec foo(Name :: atom(), Age :: integer()) -> ok.\n"));
}

#[test]
fn spec_remote_type() {
    let src = "-module(t).\n-type ref() :: pid() | atom() | {atom(), node()}.\n";
    let result = format(src);
    assert!(result.contains("-type ref() :: pid() | atom() | {atom(), node()}.\n"));
}

#[test]
fn format_fixture_type_spec_attrs() {
    let src = include_str!("../../../tests/type_spec_attrs.erl");
    let result = format(src);
    // Should contain formatted specs
    assert!(result.contains("-spec test() -> ok."));
    assert!(result.contains("-type name() :: atom()."));
    assert!(result.contains("-type byte_val() :: 0..255."));
    assert!(result.contains("-opaque internal() :: {secret, term()}."));
    // Should be idempotent
    let second = format(&result);
    assert_eq!(result, second, "type_spec_attrs formatting is not idempotent");
}

// ── Binary operator chain formatting tests ─────────────────────────

fn format_narrow(source: &str) -> String {
    let config = FormatConfig { max_width: 40, indent_size: 4 };
    nera_fmt::format_source(source, &config).unwrap()
}

#[test]
fn binop_chain_short_stays_flat() {
    let src = "-module(t).\n-export([f/0]).\nf() -> A andalso B andalso C.\n";
    let result = format(src);
    assert!(
        result.contains("A andalso B andalso C"),
        "short chain should stay flat, got:\n{}",
        result
    );
}

#[test]
fn binop_chain_long_breaks_flat() {
    // Use narrow width to force breaking
    let src = "-module(t).\n-export([f/0]).\nf() -> VeryLongA andalso VeryLongB andalso VeryLongC andalso VeryLongD.\n";
    let result = format_narrow(src);
    // Should NOT have staircase indentation (each andalso at same indent)
    assert!(
        !result.contains("        andalso"),
        "chain should not have staircase indentation, got:\n{}",
        result
    );
    // Each continuation line should start with "andalso"
    let andalso_lines: Vec<&str> = result
        .lines()
        .filter(|l| l.trim().starts_with("andalso"))
        .collect();
    assert!(
        andalso_lines.len() >= 2,
        "expected multiple andalso continuation lines, got:\n{}",
        result
    );
}

#[test]
fn binop_chain_arithmetic() {
    let src = "-module(t).\n-export([f/0]).\nf() -> VeryLongA + VeryLongB + VeryLongC + VeryLongD.\n";
    let result = format_narrow(src);
    let plus_lines: Vec<&str> = result
        .lines()
        .filter(|l| l.trim().starts_with("+ "))
        .collect();
    assert!(
        plus_lines.len() >= 2,
        "expected multiple + continuation lines, got:\n{}",
        result
    );
}

#[test]
fn binop_chain_two_element_unchanged() {
    // Two-element BinOp should use the original indented style (with nest)
    let src = "-module(t).\n-export([f/0]).\nf() -> VeryLongVariableName andalso AnotherVeryLongVariable.\n";
    let result = format_narrow(src);
    // The RHS should be indented deeper than the operator (nest adds indent)
    assert!(
        result.contains("        AnotherVeryLongVariable"),
        "two-element binop should indent RHS with nest, got:\n{}",
        result
    );
}

#[test]
fn binop_chain_idempotent() {
    let src = "-module(t).\n-export([f/0]).\nf() -> VeryLongA andalso VeryLongB andalso VeryLongC andalso VeryLongD.\n";
    let first = format_narrow(src);
    let second = format_narrow(&first);
    assert_eq!(first, second, "chain formatting is not idempotent:\n{}", first);
}

#[test]
fn binop_chain_mixed_operators_no_flatten() {
    // Mixed operators should not be flattened together
    let src = "-module(t).\n-export([f/0]).\nf() -> A andalso B orelse C.\n";
    let result = format(src);
    // Should parse and format without error
    assert!(result.contains("andalso"));
    assert!(result.contains("orelse"));
}

// ── Comment handling tests ─────────────────────────────────────────

#[test]
fn trailing_comment_on_single_clause_function() {
    let src = "-module(t).\n-export([f/0]).\nf() -> ok. % done\n";
    let result = format(src);
    assert!(
        result.contains("ok. % done"),
        "trailing comment should be inline after dot:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "trailing comment formatting not idempotent:\n{}", result);
}

#[test]
fn trailing_comment_on_multi_clause_function() {
    let src = "\
-module(t).
-export([f/1]).
f(1) -> a; % first clause
f(2) -> b. % second clause
";
    let result = format(src);
    assert!(
        result.contains("a; % first clause"),
        "first clause trailing comment should be inline after semicolon:\n{}",
        result,
    );
    assert!(
        result.contains("b. % second clause"),
        "second clause trailing comment should be inline after dot:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "multi-clause trailing comment not idempotent:\n{}", result);
}

#[test]
fn comment_between_case_clauses() {
    let src = "\
-module(t).
-export([f/1]).
f(X) ->
    case X of
        1 -> a;
        % handle two
        2 -> b;
        % default
        _ -> c
    end.
";
    let result = format(src);
    assert!(
        result.contains("% handle two"),
        "inter-clause comment was lost:\n{}",
        result,
    );
    assert!(
        result.contains("% default"),
        "inter-clause comment was lost:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "case clause comment not idempotent:\n{}", result);
}

#[test]
fn comment_between_body_expressions() {
    let src = "\
-module(t).
-export([f/0]).
f() ->
    A = 1,
    % compute B
    B = A + 1,
    B.
";
    let result = format(src);
    assert!(
        result.contains("% compute B"),
        "body comment was lost:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "body comment not idempotent:\n{}", result);
}

#[test]
fn comment_between_if_clauses() {
    let src = "\
-module(t).
-export([f/1]).
f(X) ->
    if
        X > 0 -> positive;
        % zero or negative
        true -> other
    end.
";
    let result = format(src);
    assert!(
        result.contains("% zero or negative"),
        "if clause comment was lost:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "if clause comment not idempotent:\n{}", result);
}

#[test]
fn comment_between_receive_clauses() {
    let src = "\
-module(t).
-export([f/0]).
f() ->
    receive
        {msg, Data} -> Data;
        % timeout handling
        timeout -> error
    end.
";
    let result = format(src);
    assert!(
        result.contains("% timeout handling"),
        "receive clause comment was lost:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "receive clause comment not idempotent:\n{}", result);
}

#[test]
fn standalone_comment_only_form() {
    let src = "\
-module(t).

%%% Section header
%% Description

-export([f/0]).
f() -> ok.
";
    let result = format(src);
    assert!(
        result.contains("%%% Section header"),
        "standalone comment form lost:\n{}",
        result,
    );
    assert!(
        result.contains("%% Description"),
        "standalone description comment lost:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "standalone comment form not idempotent:\n{}", result);
}

#[test]
fn leading_comment_on_record_form() {
    let src = "\
-module(t).
%% State record
-record(state, {count, name}).
";
    let result = format(src);
    assert!(
        result.contains("%% State record"),
        "record leading comment was lost:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "record leading comment not idempotent:\n{}", result);
}

#[test]
fn leading_comment_on_spec_form() {
    let src = "\
-module(t).
-export([f/1]).
%% Convert integer to binary
-spec f(integer()) -> binary().
f(X) -> integer_to_binary(X).
";
    let result = format(src);
    assert!(
        result.contains("%% Convert integer to binary\n-spec f(integer()) -> binary()."),
        "spec leading comment was lost or moved:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "spec leading comment not idempotent:\n{}", result);
}

#[test]
fn comment_between_try_catch_clauses() {
    let src = "\
-module(t).
-export([f/0]).
f() ->
    try
        foo()
    catch
        error:badarg -> badarg;
        % handle exits
        exit:Reason -> {exit, Reason}
    end.
";
    let result = format(src);
    assert!(
        result.contains("% handle exits"),
        "try/catch clause comment was lost:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "try/catch clause comment not idempotent:\n{}", result);
}

#[test]
fn trailing_comment_on_export_attribute() {
    let src = "-module(t).\n-export([f/0]). % public\nf() -> ok.\n";
    let result = format(src);
    assert!(
        result.contains("]). % public"),
        "export trailing comment should be inline:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "export trailing comment not idempotent:\n{}", result);
}

#[test]
fn trailing_comment_on_breaking_export() {
    let src = "\
-module(t).
-export([very_long_alpha/0, very_long_beta/0, very_long_gamma/0, very_long_delta/0]). % api
very_long_alpha() -> ok.
very_long_beta() -> ok.
very_long_gamma() -> ok.
very_long_delta() -> ok.
";
    let result = format(src);
    assert!(
        result.contains("]). % api"),
        "breaking export trailing comment should be on ]). line:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "breaking export trailing comment not idempotent:\n{}", result);
}

#[test]
fn trailing_comment_on_record_attribute() {
    let src = "-module(t).\n-record(state, {count, name}). % state\n";
    let result = format(src);
    assert!(
        result.contains("}). % state"),
        "record trailing comment should be inline:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "record trailing comment not idempotent:\n{}", result);
}

#[test]
fn comment_inside_map_expression() {
    let src = "\
-module(t).
-export([f/0]).
f() ->
    #{
        % name
        name => a,
        % age
        age => 1
    }.
";
    let result = format(src);
    assert!(result.contains("% name"), "map comment lost:\n{}", result);
    assert!(result.contains("% age"), "map comment lost:\n{}", result);
    let second = format(&result);
    assert_eq!(result, second, "map comment not idempotent:\n{}", result);
}

#[test]
fn comment_inside_list_expression() {
    let src = "\
-module(t).
-export([f/0]).
f() ->
    [
        % first
        alpha,
        % second
        beta
    ].
";
    let result = format(src);
    assert!(result.contains("% first"), "list comment lost:\n{}", result);
    assert!(result.contains("% second"), "list comment lost:\n{}", result);
    let second = format(&result);
    assert_eq!(result, second, "list comment not idempotent:\n{}", result);
}

#[test]
fn comment_inside_tuple_expression() {
    let src = "\
-module(t).
-export([f/0]).
f() ->
    {
        % status
        ok,
        % value
        42
    }.
";
    let result = format(src);
    assert!(result.contains("% status"), "tuple comment lost:\n{}", result);
    assert!(result.contains("% value"), "tuple comment lost:\n{}", result);
    let second = format(&result);
    assert_eq!(result, second, "tuple comment not idempotent:\n{}", result);
}

#[test]
fn comment_inside_list_comprehension() {
    let src = "\
-module(t).
-export([f/0]).
f() ->
    [X ||
        % source
        X <- [1, 2, 3],
        % filter
        X > 1
    ].
";
    let result = format(src);
    assert!(result.contains("% source"), "lc comment lost:\n{}", result);
    assert!(result.contains("% filter"), "lc comment lost:\n{}", result);
    let second = format(&result);
    assert_eq!(result, second, "lc comment not idempotent:\n{}", result);
}

#[test]
fn trailing_comment_on_spec_attribute() {
    let src = "-module(t).\n-export([f/0]).\n-spec f() -> ok. % always ok\nf() -> ok.\n";
    let result = format(src);
    assert!(
        result.contains("ok. % always ok"),
        "spec trailing comment should be inline:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "spec trailing comment not idempotent:\n{}", result);
}

// ── Regression tests: operator precedence re-parenthesization ──────

#[test]
fn regression_binop_and_with_comparison_children() {
    // Bug: `(A =:= B) and (C =:= D)` was emitted as `A =:= B and C =:= D`
    // which re-parses with different grouping because `and` binds tighter (prec 12)
    // than `=:=` (prec 6).
    let src = "-module(t).\n-export([f/2]).\nf(A, B) -> (A =:= 1) and (B =:= 2).\n";
    let result = format(src);
    assert!(
        result.contains("(A =:= 1) and (B =:= 2)"),
        "parens around comparisons in `and` must be preserved:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "binop precedence not idempotent:\n{}", result);
}

#[test]
fn regression_binop_andalso_with_orelse_child() {
    // `orelse` (prec 2) < `andalso` (prec 4): child needs parens.
    let src = "-module(t).\n-export([f/2]).\nf(A, B) -> (A orelse B) andalso true.\n";
    let result = format(src);
    assert!(
        result.contains("(A orelse B) andalso true"),
        "parens around orelse in andalso must be preserved:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "orelse/andalso precedence not idempotent:\n{}", result);
}

#[test]
fn regression_binop_higher_prec_child_no_parens() {
    // `+` (prec 10) > `==` (prec 6): child does NOT need parens.
    let src = "-module(t).\n-export([f/2]).\nf(A, B) -> A + 1 == B + 2.\n";
    let result = format(src);
    assert!(
        !result.contains("(A + 1)") && !result.contains("(B + 2)"),
        "higher-prec children should NOT get parens:\n{}",
        result,
    );
    assert!(
        result.contains("A + 1 == B + 2"),
        "expression should be flat without parens:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "higher-prec binop not idempotent:\n{}", result);
}

#[test]
fn regression_binop_mul_with_add_child() {
    // `+` (prec 10) < `*` (prec 12): child needs parens.
    let src = "-module(t).\n-export([f/2]).\nf(A, B) -> (A + B) * 2.\n";
    let result = format(src);
    assert!(
        result.contains("(A + B) * 2"),
        "parens around addition in multiplication must be preserved:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "mul/add precedence not idempotent:\n{}", result);
}

// ── Regression tests: binary segment parenthesization ──────────────

#[test]
fn regression_bin_segment_expr_value_needs_parens() {
    // Bug: `<<(X + 1):8>>` was emitted as `<<X + 1:8>>` which is invalid.
    // Only primary expressions (literals, variables) can appear bare.
    let src = "-module(t).\n-export([f/1]).\nf(X) -> <<(X + 1):8>>.\n";
    let result = format(src);
    assert!(
        result.contains("(X + 1)"),
        "non-primary expression in binary segment must be parenthesized:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "bin segment parens not idempotent:\n{}", result);
}

#[test]
fn regression_bin_segment_expr_size_needs_parens() {
    // Size expression also needs parens when non-primary.
    let src = "-module(t).\n-export([f/2]).\nf(X, N) -> <<X:(N * 8)>>.\n";
    let result = format(src);
    assert!(
        result.contains("(N * 8)"),
        "non-primary size in binary segment must be parenthesized:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "bin segment size parens not idempotent:\n{}", result);

    // NERA-121: the saved failing seed selects this real fixture. Its (-1)
    // size is folded into an integer literal, but cannot be printed bare.
    let src = include_str!("../../../tests/binary_pattern_size_overflow.erl");
    let result = format(src);
    let parsed = nera_syntax::parser::Parser::new(&result).parse_module();
    assert!(
        !parsed.has_errors(),
        "negative binary size lost its grouping: {:?}\n{}",
        parsed.errors,
        result,
    );
    assert_eq!(result, format(&result), "negative binary size is not idempotent");
}

#[test]
fn regression_bin_segment_primary_no_parens() {
    // Primary expressions (variable, integer) should NOT get extra parens.
    let src = "-module(t).\n-export([f/1]).\nf(X) -> <<X:8/integer>>.\n";
    let result = format(src);
    // Check that <<X:8...>> doesn't become <<(X):(8)...>>
    assert!(
        result.contains("<<X:8"),
        "primary expressions in binary segment should NOT get parens:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "bin segment primary not idempotent:\n{}", result);
}

// ── Regression tests: quoted atom escaping ─────────────────────────

#[test]
fn regression_atom_with_single_quote() {
    // Bug: atom `'it\'s'` was emitted as `'it's'` (unescaped), producing
    // invalid Erlang with an unmatched quote.
    let src = "-module(t).\n-export([f/0]).\nf() -> 'it\\'s'.\n";
    let result = format(src);
    assert!(
        result.contains("'it\\'s'"),
        "single quote in atom must be escaped:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "quoted atom escaping not idempotent:\n{}", result);
}

#[test]
fn regression_atom_with_backslash() {
    let src = "-module(t).\n-export([f/0]).\nf() -> 'back\\\\slash'.\n";
    let result = format(src);
    assert!(
        result.contains("'back\\\\slash'"),
        "backslash in atom must be escaped:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "backslash atom escaping not idempotent:\n{}", result);
}

#[test]
fn regression_normal_quoted_atom_unchanged() {
    // Quoted atoms that don't need escaping should pass through cleanly.
    let src = "-module(t).\n-export([f/0]).\nf() -> 'hello world'.\n";
    let result = format(src);
    assert!(
        result.contains("'hello world'"),
        "normal quoted atom should be preserved:\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "normal quoted atom not idempotent:\n{}", result);
}

// ── Regression tests: trailing comment drift ───────────────────────

#[test]
fn regression_trailing_comment_stays_on_correct_line() {
    // Bug: trailing comment on `timer_sleep(50),  %% let it exit` would
    // drift to `end,` after first format, then to `Pid2 ! stop,` after
    // second format — moving forward one expression per pass.
    let src = "\
-module(t).
-export([f/0]).
f() ->
    a(),
    b(),  %% comment on b
    c(),
    ok.
";
    let result = format(src);
    assert!(
        result.contains("b(), %% comment on b"),
        "trailing comment should stay on b():\n{}",
        result,
    );
    assert!(
        !result.contains("c(), %% comment on b"),
        "trailing comment must NOT drift to c():\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "trailing comment drift not idempotent:\n{}", result);
}

#[test]
fn regression_trailing_comment_before_try() {
    // The specific pattern from the proptest failure: trailing comment before
    // a try expression that reformats into multiple lines.
    let src = "\
-module(t).
-export([f/0]).
f() ->
    setup(),  %% initialize
    Result = try
        do_stuff()
    catch
        error:badarg -> error
    end,
    cleanup(),
    Result.
";
    let result = format(src);
    assert!(
        result.contains("setup(), %% initialize"),
        "trailing comment should stay on setup():\n{}",
        result,
    );
    let second = format(&result);
    assert_eq!(result, second, "trailing comment before try not idempotent:\n{}", result);
}
