-module(inline_hd_tl).
-export([test/0]).

%% Tests for inlined hd/1 and tl/1 (fast path: tag check + pointer load).
%% Covers: simple lists, nested structures, single-element lists,
%% deep chains, and tight loops.

test() ->
    %% Basic hd/tl
    1 = hd(id([1, 2, 3])),
    [2, 3] = tl(id([1, 2, 3])),

    %% Single-element list
    a = hd(id([a])),
    [] = tl(id([a])),

    %% Nested list
    [1, 2] = hd(id([[1, 2], [3, 4]])),
    [[3, 4]] = tl(id([[1, 2], [3, 4]])),

    %% Tuple inside list
    {a, 1} = hd(id([{a, 1}, {b, 2}])),

    %% Deep chain of hd/tl
    L = id([10, 20, 30, 40]),
    10 = hd(L),
    20 = hd(tl(L)),
    30 = hd(tl(tl(L))),
    40 = hd(tl(tl(tl(L)))),
    [] = tl(tl(tl(tl(L)))),

    %% hd/tl in a tight loop (exercises inline path under yield checks)
    ok = loop_hd(id(lists:seq(1, 1000)), 1),

    %% String (list of integers)
    104 = hd(id("hello")),
    "ello" = tl(id("hello")),

    %% Improper list
    a = hd(id([a | b])),
    b = tl(id([a | b])),

    ok.

loop_hd([], _) -> ok;
loop_hd(L, N) ->
    N = hd(L),
    loop_hd(tl(L), N + 1).

id(X) -> X.
