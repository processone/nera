%% Tests for counters module.
-module(counters_basic).
-export([test/0]).

test() ->
    %% Create a counter array with 3 slots
    Ref = counters:new(3, [atomics]),

    %% Initial values are 0
    0 = counters:get(Ref, 1),
    0 = counters:get(Ref, 2),
    0 = counters:get(Ref, 3),

    %% put sets a value
    ok = counters:put(Ref, 1, 100),
    100 = counters:get(Ref, 1),

    %% add increments
    ok = counters:add(Ref, 1, 50),
    150 = counters:get(Ref, 1),

    %% sub decrements
    ok = counters:sub(Ref, 1, 30),
    120 = counters:get(Ref, 1),

    %% add negative value
    ok = counters:add(Ref, 2, -10),
    -10 = counters:get(Ref, 2),

    %% multiple slots are independent
    ok = counters:put(Ref, 3, 999),
    120 = counters:get(Ref, 1),
    -10 = counters:get(Ref, 2),
    999 = counters:get(Ref, 3),

    %% info returns size and memory
    #{size := 3, memory := _Mem} = counters:info(Ref),

    %% multiple counter arrays are independent
    Ref2 = counters:new(2, []),
    0 = counters:get(Ref2, 1),
    ok = counters:add(Ref2, 1, 5),
    5 = counters:get(Ref2, 1),
    120 = counters:get(Ref, 1),  % original unchanged

    io:format("counters_basic: all tests passed~n"),
    ok.
