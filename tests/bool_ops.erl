%% Adapted from OTP compiler/test/andor_SUITE.erl (Apache 2.0)
%% Tests boolean and short-circuit operators.
-module(bool_ops).
-export([test/0]).

test() ->
    %% Basic boolean operators
    true = id(true) and id(true),
    false = id(true) and id(false),
    false = id(false) and id(true),
    false = id(false) and id(false),

    true = id(true) or id(true),
    true = id(true) or id(false),
    true = id(false) or id(true),
    false = id(false) or id(false),

    false = not id(true),
    true = not id(false),

    %% Short-circuit: andalso
    true = id(true) andalso id(true),
    false = id(true) andalso id(false),
    false = id(false) andalso id(true),
    false = id(false) andalso id(false),

    %% andalso should not evaluate right side if left is false
    %% (error() would crash if evaluated - short-circuit prevents it)
    false = id(false) andalso id(crash),

    %% Stricter check: the RHS is a function call that always raises.
    %% If `andalso` evaluated both sides, this would crash.
    false = id(false) andalso crash_fn(),

    %% Short-circuit: orelse
    true = id(true) orelse id(true),
    true = id(true) orelse id(false),
    true = id(false) orelse id(true),
    false = id(false) orelse id(false),

    %% orelse should not evaluate right side if left is true
    %% (error() would crash if evaluated - short-circuit prevents it)
    true = id(true) orelse id(crash),

    %% Stricter check: RHS is a function call that always raises.
    true = id(true) orelse crash_fn(),

    %% Nested boolean expressions
    true = (id(true) andalso id(true)) orelse id(false),
    true = id(false) orelse (id(true) andalso id(true)),
    false = (id(false) andalso id(true)) orelse id(false),
    false = id(false) orelse (id(false) andalso id(true)),

    %% Combinations with not
    true = not (id(false) andalso id(true)),
    false = not (id(true) orelse id(false)),

    io:format("ok~n"),
    ok.

id(I) -> I.

crash_fn() ->
    error(rhs_should_not_have_run).
