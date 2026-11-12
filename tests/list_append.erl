%% Adapted from OTP compiler test suites (Apache 2.0)
%% Tests list append (++) and subtract (--) operators.
-module(list_append).
-export([test/0]).

test() ->
    %% Basic append
    [1, 2, 3, 4] = [1, 2] ++ [3, 4],
    [1, 2, 3] = [1] ++ [2, 3],
    [1] = [] ++ [1],
    [1] = [1] ++ [],
    [] = [] ++ [],

    %% Append with different types
    [a, b, 1, 2] = [a, b] ++ [1, 2],
    [{a, 1}, {b, 2}] = [{a, 1}] ++ [{b, 2}],

    %% Multiple appends
    [1, 2, 3, 4, 5, 6] = [1, 2] ++ [3, 4] ++ [5, 6],

    %% Subtract
    [1, 3] = [1, 2, 3] -- [2],
    [1, 2, 3] = [1, 2, 3] -- [],
    [] = [1, 2, 3] -- [1, 2, 3],
    [1, 2, 3] = [1, 2, 3] -- [4, 5],
    [1, 3] = [1, 2, 3, 2] -- [2, 2],
    [1] = [1, 2, 2] -- [2, 2],

    %% Subtract removes only first occurrence
    [1, 3, 2] = [1, 2, 3, 2] -- [2],

    %% Append and subtract combined
    [1, 4] = ([1, 2] ++ [3, 4]) -- [2, 3],

    io:format("ok~n"),
    ok.
