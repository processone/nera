%% Regression test: stdlib list functions on large lists.
%% Verifies that map, filter, flatmap, flatten, zip, sort, etc.
%% work correctly on 50K+ element lists without stack overflow.
%% These were head-recursive and crashed at ~19K elements.
-module(lists_large).
-export([test/0]).

test() ->
    N = 50000,
    L = lists:seq(1, N),

    %% map on large list
    Doubled = lists:map(fun(X) -> X * 2 end, L),
    N = length(Doubled),
    2 = hd(Doubled),
    100000 = lists:last(Doubled),

    %% filter on large list
    Evens = lists:filter(fun(X) -> X rem 2 =:= 0 end, L),
    25000 = length(Evens),

    %% flatmap on large list
    Pairs = lists:flatmap(fun(X) -> [X, X] end, lists:seq(1, 10000)),
    20000 = length(Pairs),

    %% sort on large list (exercises merge)
    Rev = lists:reverse(L),
    Sorted = lists:sort(Rev),
    1 = hd(Sorted),
    N = lists:last(Sorted),

    %% zip on large list
    Zipped = lists:zip(L, Rev),
    N = length(Zipped),
    {1, N} = hd(Zipped),

    %% flatten on large flat list
    Flat = lists:flatten([lists:seq(1, 1000), lists:seq(1001, 2000)]),
    2000 = length(Flat),

    %% takewhile on large list
    Taken = lists:takewhile(fun(X) -> X =< 10000 end, L),
    10000 = length(Taken),

    %% mapfoldl on large list
    {Mapped, Sum} = lists:mapfoldl(fun(X, Acc) -> {X * 2, Acc + X} end, 0, lists:seq(1, 10000)),
    10000 = length(Mapped),
    50005000 = Sum,

    %% usort with duplicates
    Dups = lists:seq(1, 10000) ++ lists:seq(1, 10000),
    Unique = lists:usort(Dups),
    10000 = length(Unique),

    ok.
