%% Adapted from OTP compiler/test/match_SUITE.erl (Apache 2.0)
%% Tests alias patterns (= in patterns).
-module(match_alias).
-export([test/0]).

test() ->
    %% Simple alias
    {42, 42} = alias_simple(42),

    %% Alias with tuple
    {{a, b}, a} = alias_tuple({a, b}),

    %% Alias with list
    {[1, 2, 3], 1} = alias_list([1, 2, 3]),

    %% Alias in case
    {[head, tail], head} = alias_case([head, tail]),

    %% Nested alias
    {{outer, {inner, 42}}, {inner, 42}} = nested_alias({outer, {inner, 42}}),

    %% Alias in function head
    ok = alias_guard({ok, 5}),
    fail = alias_guard({ok, -1}),

    %% Multiple aliases
    {[1, 2, 3], 1, [2, 3]} = multi_alias([1, 2, 3]),

    io:format("ok~n"),
    ok.

alias_simple(X = Y) -> {X, Y}.

alias_tuple({A, _} = Whole) -> {Whole, A}.

alias_list([H | _] = Whole) -> {Whole, H}.

alias_case(L) ->
    case L of
        [H | _] = W -> {W, H}
    end.

nested_alias({_, Inner} = Whole) -> {Whole, Inner}.

alias_guard({ok, X} = _T) when X > 0 -> ok;
alias_guard(_) -> fail.

multi_alias([H | T] = Whole) -> {Whole, H, T}.
