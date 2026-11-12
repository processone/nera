%% Adapted from OTP compiler test suites (Apache 2.0)
%% Tests list-related BIFs: hd, tl, length, element, tuple_size, etc.
-module(bif_list_ops).
-export([test/0]).

test() ->
    %% hd and tl
    1 = hd([1, 2, 3]),
    a = hd([a]),
    [2, 3] = tl([1, 2, 3]),
    [] = tl([a]),

    %% length
    0 = length([]),
    1 = length([a]),
    3 = length([1, 2, 3]),
    5 = length([a, b, c, d, e]),

    %% element (1-indexed)
    a = element(1, {a, b, c}),
    b = element(2, {a, b, c}),
    c = element(3, {a, b, c}),
    42 = element(1, {42}),

    %% tuple_size
    0 = tuple_size({}),
    1 = tuple_size({a}),
    3 = tuple_size({1, 2, 3}),

    %% setelement (1-indexed)
    {x, b, c} = setelement(1, {a, b, c}, x),
    {a, x, c} = setelement(2, {a, b, c}, x),
    {a, b, x} = setelement(3, {a, b, c}, x),

    %% byte_size
    0 = byte_size(<<>>),
    3 = byte_size(<<1, 2, 3>>),
    5 = byte_size(<<0, 0, 0, 0, 0>>),

    %% abs
    5 = abs(5),
    5 = abs(-5),
    0 = abs(0),

    %% max and min
    5 = max(3, 5),
    5 = max(5, 3),
    3 = min(3, 5),
    3 = min(5, 3),

    %% map_size
    0 = map_size(#{}),
    2 = map_size(#{a => 1, b => 2}),

    io:format("ok~n"),
    ok.
