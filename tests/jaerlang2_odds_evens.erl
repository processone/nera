%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (lib_misc.erl)
%% Tests: list comprehensions with rem, accumulator pattern, perms with nested LC
-module(jaerlang2_odds_evens).
-export([test/0]).

test() ->
    %% Test odds_and_evens using list comprehensions (lib_misc:odds_and_evens1)
    L = [1, 2, 3, 4, 5, 6, 7, 8],
    {Odds1, Evens1} = odds_and_evens1(L),
    [1, 3, 5, 7] = Odds1,
    [2, 4, 6, 8] = Evens1,

    %% Test odds_and_evens using accumulator (lib_misc:odds_and_evens2)
    {Odds2, Evens2} = odds_and_evens2(L),
    %% Accumulator version produces reversed lists
    [7, 5, 3, 1] = Odds2,
    [8, 6, 4, 2] = Evens2,

    %% Empty list
    {[], []} = odds_and_evens1([]),
    {[], []} = odds_and_evens2([]),

    %% Test permutations (lib_misc:perms)
    [[1, 2, 3], [1, 3, 2], [2, 1, 3], [2, 3, 1], [3, 1, 2], [3, 2, 1]] = perms([1, 2, 3]),

    %% perms of empty list
    [[]] = perms([]),

    %% perms of single element
    [[a]] = perms([a]),

    %% perms of two elements
    [[a, b], [b, a]] = perms([a, b]),

    %% Test qsort (lib_misc:qsort)
    [] = qsort([]),
    [1] = qsort([1]),
    [1, 2, 3, 4, 5] = qsort([3, 1, 4, 5, 2]),
    [1, 1, 2, 3, 3] = qsort([3, 1, 3, 2, 1]),

    ok.

%% List comprehension version
odds_and_evens1(L) ->
    Odds  = [X || X <- L, (X rem 2) =:= 1],
    Evens = [X || X <- L, (X rem 2) =:= 0],
    {Odds, Evens}.

%% Accumulator version
odds_and_evens2(L) ->
    odds_and_evens_acc(L, [], []).

odds_and_evens_acc([H | T], Odds, Evens) ->
    case (H rem 2) of
        1 -> odds_and_evens_acc(T, [H | Odds], Evens);
        0 -> odds_and_evens_acc(T, Odds, [H | Evens])
    end;
odds_and_evens_acc([], Odds, Evens) ->
    {Odds, Evens}.

%% Permutations using nested list comprehensions
perms([]) -> [[]];
perms(L)  -> [[H | T] || H <- L, T <- perms(L -- [H])].

%% Quicksort using list comprehensions
qsort([]) -> [];
qsort([Pivot | T]) ->
    qsort([X || X <- T, X < Pivot])
    ++ [Pivot] ++
    qsort([X || X <- T, X >= Pivot]).
