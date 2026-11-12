%% Adapted from OTP compiler/test/match_SUITE.erl (Apache 2.0)
%% Tests list pattern matching and list operations.
-module(match_list).
-export([test/0]).

test() ->
    %% Basic cons matching
    {1, [2, 3]} = split([1, 2, 3]),
    {a, []} = split([a]),

    %% Multiple element matching
    {1, 2, [3, 4]} = split2([1, 2, 3, 4]),

    %% Exact list matching
    ok = exact_list([1, 2, 3]),
    fail = exact_list([1, 2]),
    fail = exact_list([1, 2, 3, 4]),

    %% Empty list
    empty = classify([]),
    singleton = classify([99]),
    many = classify([1, 2, 3]),

    %% List construction and matching
    [1, 2, 3] = [1 | [2 | [3 | []]]],
    [1, 2, 3] = [1, 2] ++ [3],
    [1, 2, 3, 4] = [1, 2] ++ [3, 4],

    %% Nested list patterns
    [[1, 2], [3, 4]] = id([[1, 2], [3, 4]]),
    [1, 2] = hd([[1, 2], [3, 4]]),
    [[3, 4]] = tl([[1, 2], [3, 4]]),

    %% List in tuple matching
    {ok, [1, 2, 3]} = id({ok, [1, 2, 3]}),

    %% Pattern matching with literal lists
    ok = match_literal([a, b, c]),
    fail = match_literal([a, b]),

    %% List length in guards
    short = by_length([1]),
    short = by_length([1, 2]),
    long = by_length([1, 2, 3, 4]),

    %% List subtraction
    [1, 3] = [1, 2, 3] -- [2],
    [1, 2, 3] = [1, 2, 3] -- [],
    [] = [1, 2, 3] -- [1, 2, 3],

    io:format("ok~n"),
    ok.

split([H | T]) -> {H, T}.

split2([A, B | T]) -> {A, B, T}.

exact_list([_, _, _]) -> ok;
exact_list(_) -> fail.

classify([]) -> empty;
classify([_]) -> singleton;
classify([_, _ | _]) -> many.

match_literal([a, b, c]) -> ok;
match_literal(_) -> fail.

by_length(L) when length(L) =< 2 -> short;
by_length(_) -> long.

id(I) -> I.
