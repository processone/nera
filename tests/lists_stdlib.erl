%% Tests stdlib lists module functions implemented in Erlang:
%% map, filter, foldl, foldr, foreach, all, any, flatmap, partition,
%% takewhile, dropwhile, sort, flatten, zip, unzip, seq, duplicate,
%% keydelete, keyreplace, keystore, usort, max, min, sum, splitwith, mapfoldl.
-module(lists_stdlib).
-export([test/0]).

id(I) -> I.

test() ->
    %% map/2
    [] = lists:map(fun(X) -> X * 2 end, []),
    [2, 4, 6] = lists:map(fun(X) -> X * 2 end, [1, 2, 3]),
    [a, b] = lists:map(fun({K, _V}) -> K end, [{a, 1}, {b, 2}]),

    %% filter/2
    [] = lists:filter(fun(_) -> true end, []),
    [2, 4] = lists:filter(fun(X) -> X rem 2 =:= 0 end, [1, 2, 3, 4, 5]),
    [1, 2, 3] = lists:filter(fun(_) -> true end, [1, 2, 3]),

    %% foldl/3
    0 = lists:foldl(fun(X, Acc) -> X + Acc end, 0, []),
    6 = lists:foldl(fun(X, Acc) -> X + Acc end, 0, [1, 2, 3]),
    [3, 2, 1] = lists:foldl(fun(X, Acc) -> [X|Acc] end, [], [1, 2, 3]),

    %% foldr/3
    [1, 2, 3] = lists:foldr(fun(X, Acc) -> [X|Acc] end, [], [1, 2, 3]),

    %% foreach/2
    ok = lists:foreach(fun(_) -> ok end, []),
    ok = lists:foreach(fun(_) -> ok end, [1, 2, 3]),

    %% all/2
    true = lists:all(fun(X) -> X > 0 end, [1, 2, 3]),
    false = lists:all(fun(X) -> X > 0 end, [1, -2, 3]),
    true = lists:all(fun(_) -> true end, []),

    %% any/2
    true = lists:any(fun(X) -> X > 2 end, [1, 2, 3]),
    false = lists:any(fun(X) -> X > 5 end, [1, 2, 3]),
    false = lists:any(fun(_) -> true end, []),

    %% flatmap/2
    [] = lists:flatmap(fun(X) -> [X, X] end, []),
    [1, 1, 2, 2, 3, 3] = lists:flatmap(fun(X) -> [X, X] end, [1, 2, 3]),

    %% partition/2
    {[2, 4], [1, 3, 5]} = lists:partition(fun(X) -> X rem 2 =:= 0 end, [1, 2, 3, 4, 5]),
    {[], []} = lists:partition(fun(_) -> true end, []),

    %% takewhile/2
    [1, 2] = lists:takewhile(fun(X) -> X < 3 end, [1, 2, 3, 4]),
    [] = lists:takewhile(fun(X) -> X < 0 end, [1, 2, 3]),
    [1, 2, 3] = lists:takewhile(fun(_) -> true end, [1, 2, 3]),

    %% dropwhile/2
    [3, 4] = lists:dropwhile(fun(X) -> X < 3 end, [1, 2, 3, 4]),
    [1, 2, 3] = lists:dropwhile(fun(X) -> X < 0 end, [1, 2, 3]),
    [] = lists:dropwhile(fun(_) -> true end, [1, 2, 3]),

    %% sort/1
    [] = lists:sort([]),
    [1] = lists:sort([1]),
    [1, 2, 3] = lists:sort([3, 1, 2]),
    [1, 2, 3, 4, 5] = lists:sort([5, 3, 1, 4, 2]),
    [1, 1, 2, 3] = lists:sort([3, 1, 2, 1]),

    %% sort/2 (custom comparison)
    [3, 2, 1] = lists:sort(fun(A, B) -> A >= B end, [1, 2, 3]),

    %% flatten/1
    [] = lists:flatten([]),
    [1, 2, 3] = lists:flatten([1, 2, 3]),
    [1, 2, 3, 4] = lists:flatten([[1, 2], [3, 4]]),
    [1, 2, 3] = lists:flatten([[1], [[2]], [[[3]]]]),

    %% zip/2
    [] = lists:zip([], []),
    [{a, 1}, {b, 2}] = lists:zip([a, b], [1, 2]),

    %% unzip/1
    {[], []} = lists:unzip([]),
    {[a, b], [1, 2]} = lists:unzip([{a, 1}, {b, 2}]),

    %% seq/2,3
    [1, 2, 3] = lists:seq(1, 3),
    [1] = lists:seq(1, 1),
    [] = lists:seq(1, 0),
    [0, 2, 4] = lists:seq(0, 4, 2),
    [5, 3, 1] = lists:seq(5, 1, -2),

    %% duplicate/2
    [] = lists:duplicate(0, a),
    [a, a, a] = lists:duplicate(3, a),

    %% append/2
    [1, 2, 3, 4] = lists:append([1, 2], [3, 4]),
    [1, 2] = lists:append([], [1, 2]),

    %% subtract/2
    [1, 3] = lists:subtract([1, 2, 3], [2]),

    %% keydelete/3
    [{b, 2}] = lists:keydelete(a, 1, [{a, 1}, {b, 2}]),
    [{a, 1}, {b, 2}] = lists:keydelete(c, 1, [{a, 1}, {b, 2}]),

    %% keyreplace/4
    [{x, 0}, {b, 2}] = lists:keyreplace(a, 1, [{a, 1}, {b, 2}], {x, 0}),
    [{a, 1}, {b, 2}] = lists:keyreplace(c, 1, [{a, 1}, {b, 2}], {x, 0}),

    %% keystore/4
    [{x, 0}, {b, 2}] = lists:keystore(a, 1, [{a, 1}, {b, 2}], {x, 0}),
    [{a, 1}, {b, 2}, {c, 3}] = lists:keystore(c, 1, [{a, 1}, {b, 2}], {c, 3}),

    %% usort/1
    [1, 2, 3] = lists:usort([3, 1, 2, 1, 3]),
    [] = lists:usort([]),

    %% max/1, min/1
    3 = lists:max([1, 3, 2]),
    1 = lists:min([3, 1, 2]),

    %% sum/1
    0 = lists:sum([]),
    6 = lists:sum([1, 2, 3]),

    %% splitwith/2
    {[1, 2], [3, 4, 5]} = lists:splitwith(fun(X) -> X < 3 end, [1, 2, 3, 4, 5]),
    {[], [1, 2]} = lists:splitwith(fun(X) -> X < 0 end, [1, 2]),

    %% mapfoldl/3
    {[2, 4, 6], 6} = lists:mapfoldl(fun(X, Acc) -> {X * 2, Acc + X} end, 0, [1, 2, 3]),
    {[], 0} = lists:mapfoldl(fun(X, Acc) -> {X, Acc} end, 0, []),

    io:format("ok~n"),
    ok.
