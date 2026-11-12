%% Adapted from OTP compiler/test/match_SUITE.erl (Apache 2.0)
%% Tests deeply nested and complex pattern matching scenarios.
-module(match_nested).
-export([test/0]).

test() ->
    %% Nested tuple in list
    [{a, 1}, {b, 2}] = id([{a, 1}, {b, 2}]),
    {a, 1} = hd([{a, 1}, {b, 2}]),

    %% List of lists
    [[1, 2], [3, 4]] = id([[1, 2], [3, 4]]),

    %% Deep tuple nesting
    {a, {b, {c, 42}}} = id({a, {b, {c, 42}}}),
    42 = deep_extract({a, {b, {c, 42}}}),

    %% Complex function clause matching
    {sum, 3} = process({add, 1, 2}),
    {product, 6} = process({mul, 2, 3}),
    {head, a} = process({first, [a, b, c]}),

    %% Pattern matching with multiple bindings
    {1, 2, 3, 6} = multi_bind({1, 2, 3}),

    %% Overlapping patterns (first match wins)
    specific = overlap({a, 1}),
    general_a = overlap({a, 2}),
    general = overlap({b, 1}),

    %% Pattern match in let-binding style
    Result = begin
        {X, Y} = {10, 20},
        X + Y
    end,
    30 = Result,

    io:format("ok~n"),
    ok.

deep_extract({_, {_, {_, V}}}) -> V.

process({add, A, B}) -> {sum, A + B};
process({mul, A, B}) -> {product, A * B};
process({first, [H | _]}) -> {head, H}.

multi_bind({A, B, C}) -> {A, B, C, A + B + C}.

overlap({a, 1}) -> specific;
overlap({a, _}) -> general_a;
overlap({_, _}) -> general.

id(I) -> I.
