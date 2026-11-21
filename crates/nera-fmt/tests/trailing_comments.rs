use nera_fmt::{config::FormatConfig, format_source};
use nera_syntax::lexer::Lexer;
use std::{fs, path::Path};

fn format(source: &str) -> String {
    format_source(source, &FormatConfig::default()).unwrap()
}

// Already formatted code must come back unchanged, with every comment on the
// line of the code it follows.
fn assert_unchanged(source: &str) {
    assert_eq!(format(source), source);
}

fn comments(source: &str) -> Vec<String> {
    let lexed = Lexer::lex_all_with_comments(source).unwrap();
    // The formatter strips spaces at line ends, including a comment's.
    let mut texts: Vec<String> = lexed
        .comments
        .into_iter()
        .map(|(text, _)| text.trim_end().to_string())
        .collect();
    texts.sort();
    texts
}

#[test]
fn clause_comments_stay_with_their_clause() {
    // The shape of queue:split/2, whose comments used to trade clauses.
    assert_unchanged(
        "split(N, Q) ->
    if
        N < 1 -> % Lf >= 2
            a(N);
        N > 1 ->
            M = b(N),
            if
                M < 0 -> erlang:error(badarg, [N, Q]);
                true -> % M == 0
                    {Q, {[], []}}
            end;
        true -> % N == Lf
            {f2r(Q), r2f(Q)}
    end;
split(N, Q) -> erlang:error(badarg, [N, Q]).
",
    );
}

#[test]
fn comment_after_a_clause_separator_stays_on_its_clause() {
    assert_unchanged(
        "counter(N) ->
    receive
        {From, tick} ->
            From ! {self(), N}, %% (3)
            counter(N); %% (4)
        stop -> true
    end.
",
    );
}

#[test]
fn every_element_keeps_its_comment() {
    assert_unchanged(
        "f() ->
    [
        1, % one
        2, % two
        3 % three
    ].
",
    );
}

#[test]
fn commented_binary_keeps_its_lines() {
    assert_unchanged(
        "packet() ->
    <<
        %% IPv4 header (20 bytes)
        69, 0, 0, 60, %% ver=4, ihl=5, len=60
        28, 70, 64, 0, %% id, flags=DF, frag=0
        %% TCP header (20 bytes)
        192, 1, 0, 80 %% src_port=49153, dst_port=80
    >>.
",
    );
}

#[test]
fn comments_stay_after_block_keywords() {
    assert_unchanged(
        "a(X) -> % after arrow
    try % after try
        X
    of % after of
        1 -> one; % clause one
        _ -> % other
            other
    catch % after catch
        error:_ -> err % caught
    after % after after
        ok % done
    end,
    case X of % after case
        1 ->
            foo(a, % first arg
                b); % second arg
        2 ->
            [Y ||
                Y <- [1], % generator
                Y > 0] % filter
    end,
    receive % waiting
        z -> ok % zed
    after
        10 -> % timer
            ok
    end.
",
    );
}

#[test]
fn attributes_keep_comments_between_their_tokens() {
    assert_unchanged(
        "-export([a/0, % first export
         b/0]). % second export
-record(r, {x, % field x
            y = 1 % field y
           }).
-type t() :: a | % alternative a
             b.
",
    );
}

#[test]
fn comment_holding_a_dot_stays_after_its_form() {
    assert_unchanged("-define(X, 1). % see section 4.2. Do not change.\n");
}

#[test]
fn repository_sources_keep_comments_and_format_stably() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for file in [
        "demo/serverless/handler.erl",
        "erlang/stdlib/queue.erl",
        "erlang/stdlib/rand.erl",
        "examples/jaerlang2/counter3.erl",
        "examples/jaerlang2/multi_server.erl",
        "examples/jaerlang2/socket_examples.erl",
        "examples/jaerlang2/wordcount.erl",
    ] {
        let source = fs::read_to_string(root.join(file)).unwrap();
        let formatted = format(&source);
        assert_eq!(format(&formatted), formatted, "{file}: a second format changed it");
        assert_eq!(comments(&formatted), comments(&source), "{file}: comments changed");
    }
}
