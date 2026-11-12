%% Tests lists module BIFs: reverse, member, keyfind, keymember, keysearch,
%% append, last, nth.
-module(lists_bifs).
-export([test/0]).

id(I) -> I.

test() ->
    %% lists:reverse/1
    [] = lists:reverse([]),
    [1] = lists:reverse([1]),
    [3, 2, 1] = lists:reverse([1, 2, 3]),
    [c, b, a] = lists:reverse(id([a, b, c])),

    %% lists:reverse/2
    [3, 2, 1] = lists:reverse([1, 2, 3], []),
    [3, 2, 1, a, b] = lists:reverse([1, 2, 3], [a, b]),
    [a, b] = lists:reverse([], [a, b]),

    %% lists:member/2
    true = lists:member(a, [a, b, c]),
    true = lists:member(b, [a, b, c]),
    true = lists:member(c, [a, b, c]),
    false = lists:member(d, [a, b, c]),
    false = lists:member(a, []),
    true = lists:member(2, id([1, 2, 3])),

    %% lists:keyfind/3
    {a, 1} = lists:keyfind(a, 1, [{a, 1}, {b, 2}, {c, 3}]),
    {b, 2} = lists:keyfind(b, 1, [{a, 1}, {b, 2}, {c, 3}]),
    false = lists:keyfind(d, 1, [{a, 1}, {b, 2}, {c, 3}]),
    %% Search by second element
    {b, 2} = lists:keyfind(2, 2, [{a, 1}, {b, 2}, {c, 3}]),
    false = lists:keyfind(99, 2, [{a, 1}, {b, 2}]),
    %% Empty list
    false = lists:keyfind(a, 1, []),
    %% Tuples of different sizes
    {x, y, z} = lists:keyfind(x, 1, [{a, b}, {x, y, z}, {c, d}]),

    %% lists:keymember/3
    true = lists:keymember(a, 1, [{a, 1}, {b, 2}]),
    true = lists:keymember(b, 1, [{a, 1}, {b, 2}]),
    false = lists:keymember(c, 1, [{a, 1}, {b, 2}]),
    false = lists:keymember(a, 1, []),

    %% lists:keysearch/3
    {value, {a, 1}} = lists:keysearch(a, 1, [{a, 1}, {b, 2}]),
    {value, {b, 2}} = lists:keysearch(b, 1, [{a, 1}, {b, 2}]),
    false = lists:keysearch(c, 1, [{a, 1}, {b, 2}]),

    %% lists:append/1
    [] = lists:append([]),
    [1, 2, 3] = lists:append([[1, 2, 3]]),
    [1, 2, 3, 4, 5, 6] = lists:append([[1, 2], [3, 4], [5, 6]]),
    [a, b, c] = lists:append([[a], [b], [c]]),
    [1, 2] = lists:append([[], [1, 2], []]),
    [] = lists:append([[], [], []]),

    %% lists:last/1
    1 = lists:last([1]),
    3 = lists:last([1, 2, 3]),
    c = lists:last(id([a, b, c])),

    %% lists:nth/2 (1-indexed)
    a = lists:nth(1, [a, b, c]),
    b = lists:nth(2, [a, b, c]),
    c = lists:nth(3, [a, b, c]),
    1 = lists:nth(1, id([1, 2, 3])),

    io:format("ok~n"),
    ok.
