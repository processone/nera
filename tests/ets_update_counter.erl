-module(ets_update_counter).
-export([test/0]).

test() ->
    %% Basic update_counter/3 with bare integer increment
    T = ets:new(counters, [set, public]),
    ets:insert(T, {key1, 0}),
    1 = ets:update_counter(T, key1, 1),
    2 = ets:update_counter(T, key1, 1),
    5 = ets:update_counter(T, key1, 3),

    %% Negative increment (decrement)
    4 = ets:update_counter(T, key1, -1),

    %% Verify via lookup
    [{key1, 4}] = ets:lookup(T, key1),

    %% {Pos, Incr} tuple form — update element at position 3
    ets:insert(T, {key2, 10, 20, 30}),
    25 = ets:update_counter(T, key2, {3, 5}),
    [{key2, 10, 25, 30}] = ets:lookup(T, key2),

    %% {Pos, Incr, Threshold, SetValue} — wrap-around
    ets:insert(T, {wrap, 0}),
    1 = ets:update_counter(T, wrap, {2, 1, 5, 0}),
    2 = ets:update_counter(T, wrap, {2, 1, 5, 0}),
    3 = ets:update_counter(T, wrap, {2, 1, 5, 0}),
    4 = ets:update_counter(T, wrap, {2, 1, 5, 0}),
    5 = ets:update_counter(T, wrap, {2, 1, 5, 0}),
    %% Next increment would exceed threshold 5, so reset to 0
    0 = ets:update_counter(T, wrap, {2, 1, 5, 0}),

    %% Custom keypos
    T2 = ets:new(kp_test, [set, public, {keypos, 2}]),
    ets:insert(T2, {100, mykey, 50}),
    %% Default position = keypos + 1 = 3
    55 = ets:update_counter(T2, mykey, 5),
    [{100, mykey, 55}] = ets:lookup(T2, mykey),

    %% Stress: many increments on same key (simulates OneCached stats)
    ets:insert(T, {stats, 0}),
    N = 10000,
    stress_loop(T, N),
    [{stats, N}] = ets:lookup(T, stats),

    ets:delete(T),
    ets:delete(T2),
    ok.

stress_loop(_, 0) -> ok;
stress_loop(T, N) ->
    ets:update_counter(T, stats, 1),
    stress_loop(T, N - 1).
