-module(list_comp).
-export([start/0]).

start() ->
    %% Simple list comprehension: double each element
    Doubles = [X * 2 || X <- [1, 2, 3, 4, 5]],
    io:format("Doubles: ~w~n", [Doubles]),

    %% List comprehension with filter: only even numbers
    Evens = [X || X <- [1, 2, 3, 4, 5, 6], X rem 2 =:= 0],
    io:format("Evens: ~w~n", [Evens]),

    %% Squares of odd numbers
    OddSquares = [X * X || X <- [1, 2, 3, 4, 5], X rem 2 =:= 1],
    io:format("Odd squares: ~w~n", [OddSquares]),

    ok.
