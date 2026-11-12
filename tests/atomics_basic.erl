%% Tests for atomics module.
-module(atomics_basic).
-export([test/0]).

test() ->
    %% Create signed atomic array with 3 slots
    Ref = atomics:new(3, [{signed, true}]),

    %% Initial values are 0
    0 = atomics:get(Ref, 1),
    0 = atomics:get(Ref, 2),
    0 = atomics:get(Ref, 3),

    %% put and get
    ok = atomics:put(Ref, 1, 42),
    42 = atomics:get(Ref, 1),

    %% add
    ok = atomics:add(Ref, 1, 8),
    50 = atomics:get(Ref, 1),

    %% sub
    ok = atomics:sub(Ref, 1, 20),
    30 = atomics:get(Ref, 1),

    %% add_get returns new value
    35 = atomics:add_get(Ref, 1, 5),

    %% sub_get returns new value
    25 = atomics:sub_get(Ref, 1, 10),

    %% exchange returns old value
    25 = atomics:exchange(Ref, 1, 100),
    100 = atomics:get(Ref, 1),

    %% compare_exchange success
    ok = atomics:compare_exchange(Ref, 1, 100, 200),
    200 = atomics:get(Ref, 1),

    %% compare_exchange failure — returns current value
    200 = atomics:compare_exchange(Ref, 1, 999, 300),
    200 = atomics:get(Ref, 1),  % unchanged

    %% negative values (signed)
    ok = atomics:put(Ref, 2, -100),
    -100 = atomics:get(Ref, 2),

    %% slots are independent
    ok = atomics:put(Ref, 3, 777),
    200 = atomics:get(Ref, 1),
    -100 = atomics:get(Ref, 2),
    777 = atomics:get(Ref, 3),

    %% info returns map
    #{size := 3, memory := _Mem} = atomics:info(Ref),

    %% unsigned array
    URef = atomics:new(2, [{signed, false}]),
    ok = atomics:put(URef, 1, 42),
    42 = atomics:get(URef, 1),

    io:format("atomics_basic: all tests passed~n"),
    ok.
