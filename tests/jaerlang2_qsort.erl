%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (lib_misc.erl)
%% Tests: quicksort with list comprehensions, ++ operator, guards in LC filters
-module(jaerlang2_qsort).
-export([test/0]).

test() ->
    [] = qsort([]),
    [1] = qsort([1]),
    [1, 2, 3] = qsort([3, 1, 2]),
    [1, 2, 3, 4, 5] = qsort([5, 3, 1, 4, 2]),
    [1, 1, 2, 3, 3] = qsort([3, 1, 3, 2, 1]),
    [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] = qsort([10, 9, 8, 7, 6, 5, 4, 3, 2, 1]),
    ok.

qsort([]) -> [];
qsort([Pivot|T]) ->
    qsort([X || X <- T, X < Pivot])
    ++ [Pivot] ++
    qsort([X || X <- T, X >= Pivot]).
