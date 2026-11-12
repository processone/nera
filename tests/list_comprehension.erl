%% Adapted from OTP compiler/test/lc_SUITE.erl (Apache 2.0)
%% Tests list comprehensions.
%% See also lc_nested.erl for nested generator tests.
-module(list_comprehension).
-export([test/0]).

test() ->
    %% Basic list comprehension
    [2, 4, 6] = [X * 2 || X <- [1, 2, 3]],

    %% With filter
    [2, 4] = [X || X <- [1, 2, 3, 4, 5], X rem 2 == 0],

    %% Square numbers
    [1, 4, 9, 16] = [X * X || X <- [1, 2, 3, 4]],

    %% Empty source
    [] = [X || X <- []],

    %% All filtered out
    [] = [X || X <- [1, 3, 5], X rem 2 == 0],

    %% Complex expressions in body
    [3, 5, 7] = [X + Y || {X, Y} <- [{1, 2}, {2, 3}, {3, 4}]],

    io:format("ok~n"),
    ok.
