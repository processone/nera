%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (lib_misc.erl)
%% Tests: for loop as recursive function, higher-order functions
-module(jaerlang2_for).
-export([test/0]).

test() ->
    %% for generates a list by applying F to each integer in range
    [1, 2, 3, 4, 5] = for(1, 5, fun(I) -> I end),
    [1, 4, 9, 16, 25] = for(1, 5, fun(I) -> I * I end),
    [10] = for(10, 10, fun(I) -> I end),

    %% odds_and_evens using list comprehensions
    {[1, 3, 5], [2, 4]} = odds_and_evens([1, 2, 3, 4, 5]),
    {[], []} = odds_and_evens([]),
    {[1], []} = odds_and_evens([1]),

    %% odds_and_evens using accumulator
    %% Note: accumulator version returns reversed order
    {[5, 3, 1], [4, 2]} = odds_and_evens_acc([1, 2, 3, 4, 5]),

    ok.

for(Max, Max, F) -> [F(Max)];
for(I, Max, F)   -> [F(I)|for(I+1, Max, F)].

%% List comprehension version
odds_and_evens(L) ->
    Odds  = [X || X <- L, (X rem 2) =:= 1],
    Evens = [X || X <- L, (X rem 2) =:= 0],
    {Odds, Evens}.

%% Accumulator version
odds_and_evens_acc(L) ->
    odds_and_evens_acc(L, [], []).

odds_and_evens_acc([H|T], Odds, Evens) ->
    case (H rem 2) of
        1 -> odds_and_evens_acc(T, [H|Odds], Evens);
        0 -> odds_and_evens_acc(T, Odds, [H|Evens])
    end;
odds_and_evens_acc([], Odds, Evens) ->
    {Odds, Evens}.
