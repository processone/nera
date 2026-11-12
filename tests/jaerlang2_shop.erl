%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (shop.erl + shop1.erl)
%% Tests: atom pattern matching, list recursion over tuples, multi-clause dispatch
-module(jaerlang2_shop).
-export([test/0]).

test() ->
    %% cost lookup
    5 = cost(oranges),
    8 = cost(newspaper),
    2 = cost(apples),
    9 = cost(pears),
    7 = cost(milk),

    %% shopping total
    0 = total([]),
    10 = total([{oranges, 2}]),
    31 = total([{oranges, 2}, {newspaper, 1}, {apples, 3}, {milk, 1}]),

    ok.

cost(oranges)   -> 5;
cost(newspaper) -> 8;
cost(apples)    -> 2;
cost(pears)     -> 9;
cost(milk)      -> 7.

total([{What, N}|T]) -> cost(What) * N + total(T);
total([])            -> 0.
