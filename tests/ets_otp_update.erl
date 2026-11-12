%% Tests ported from OTP ets_SUITE.erl: update_counter and update_element.
%% Adapted to nera test convention (test/0 -> ok, badmatch = failure).
-module(ets_otp_update).
-export([test/0]).

test() ->
    test_update_counter_basic(),
    test_update_counter_positions(),
    test_update_counter_wrap(),
    test_update_counter_negative(),
    test_update_counter_stress(),
    test_update_element_basic(),
    test_update_element_missing(),
    ok.

%% OTP update_counter_for: basic increment forms
test_update_counter_basic() ->
    T = ets:new(t, [set, public]),
    ets:insert(T, {a, 0, 0, 0, 0}),

    %% Bare integer increment (default pos = keypos+1 = 2)
    100 = ets:update_counter(T, a, 100),
    [{a, 100, 0, 0, 0}] = ets:lookup(T, a),

    %% Another increment
    200 = ets:update_counter(T, a, 100),
    [{a, 200, 0, 0, 0}] = ets:lookup(T, a),

    %% Increment by 1
    201 = ets:update_counter(T, a, 1),

    ets:delete(T),
    ok.

%% OTP update_counter_for: {Pos, Incr} on different positions
test_update_counter_positions() ->
    T = ets:new(t, [set, public]),
    ets:insert(T, {key, 0, 0, 0, 0}),

    %% Position 2 (default)
    10 = ets:update_counter(T, key, {2, 10}),
    %% Position 3
    20 = ets:update_counter(T, key, {3, 20}),
    %% Position 4
    30 = ets:update_counter(T, key, {4, 30}),
    %% Position 5
    40 = ets:update_counter(T, key, {5, 40}),

    [{key, 10, 20, 30, 40}] = ets:lookup(T, key),

    %% Increment position 3 again
    25 = ets:update_counter(T, key, {3, 5}),
    [{key, 10, 25, 30, 40}] = ets:lookup(T, key),

    ets:delete(T),
    ok.

%% OTP update_counter_for: {Pos, Incr, Threshold, SetValue} wrap semantics
test_update_counter_wrap() ->
    T = ets:new(t, [set, public]),

    %% Positive wrap: when value + incr > threshold, set to SetValue
    ets:insert(T, {w1, 0}),
    1 = ets:update_counter(T, w1, {2, 1, 3, 0}),
    2 = ets:update_counter(T, w1, {2, 1, 3, 0}),
    3 = ets:update_counter(T, w1, {2, 1, 3, 0}),
    %% 3 + 1 = 4 > 3 → wrap to 0
    0 = ets:update_counter(T, w1, {2, 1, 3, 0}),
    %% Continue from 0
    1 = ets:update_counter(T, w1, {2, 1, 3, 0}),

    %% Wrap with increment > 1
    ets:insert(T, {w2, 0}),
    5 = ets:update_counter(T, w2, {2, 5, 10, 0}),
    10 = ets:update_counter(T, w2, {2, 5, 10, 0}),
    %% 10 + 5 = 15 > 10 → wrap to 0
    0 = ets:update_counter(T, w2, {2, 5, 10, 0}),

    %% Negative wrap: when value + incr < threshold, set to SetValue
    ets:insert(T, {w3, 10}),
    7 = ets:update_counter(T, w3, {2, -3, 0, 10}),
    4 = ets:update_counter(T, w3, {2, -3, 0, 10}),
    1 = ets:update_counter(T, w3, {2, -3, 0, 10}),
    %% 1 + (-3) = -2 < 0 → wrap to 10
    10 = ets:update_counter(T, w3, {2, -3, 0, 10}),

    ets:delete(T),
    ok.

%% OTP: negative increments, crossing zero
test_update_counter_negative() ->
    T = ets:new(t, [set, public]),
    ets:insert(T, {neg, 100}),

    %% Decrement
    90 = ets:update_counter(T, neg, -10),
    80 = ets:update_counter(T, neg, -10),

    %% Decrement past zero
    0 = ets:update_counter(T, neg, -80),
    -5 = ets:update_counter(T, neg, -5),
    [{neg, -5}] = ets:lookup(T, neg),

    %% Back to positive
    5 = ets:update_counter(T, neg, 10),

    ets:delete(T),
    ok.

%% OTP update_counter_table_growth: 10K sequential increments
test_update_counter_stress() ->
    T = ets:new(t, [set, public]),
    ets:insert(T, {counter, 0}),

    N = 10000,
    stress_incr(T, N),

    [{counter, N}] = ets:lookup(T, counter),

    ets:delete(T),
    ok.

%% OTP update_element: single and multi-position updates
test_update_element_basic() ->
    T = ets:new(t, [set, public]),
    ets:insert(T, {key1, "a", "b", "c"}),

    %% Single position update
    true = ets:update_element(T, key1, {2, "A"}),
    [{key1, "A", "b", "c"}] = ets:lookup(T, key1),

    %% Another position
    true = ets:update_element(T, key1, {4, "C"}),
    [{key1, "A", "b", "C"}] = ets:lookup(T, key1),

    %% Size unchanged
    1 = ets:info(T, size),

    %% With custom keypos
    T2 = ets:new(t2, [set, public, {keypos, 2}]),
    ets:insert(T2, {"phone", alice, "alice@example.com"}),
    true = ets:update_element(T2, alice, {3, "new@example.com"}),
    [{"phone", alice, "new@example.com"}] = ets:lookup(T2, alice),
    true = ets:update_element(T2, alice, {1, "mobile"}),
    [{"mobile", alice, "new@example.com"}] = ets:lookup(T2, alice),

    ets:delete(T),
    ets:delete(T2),
    ok.

%% OTP update_element: missing key returns false
test_update_element_missing() ->
    T = ets:new(t, [set, public]),
    ets:insert(T, {exists, "val"}),

    %% Existing key → true
    true = ets:update_element(T, exists, {2, "new"}),

    %% Missing key → false
    false = ets:update_element(T, missing, {2, "val"}),

    %% Table unchanged
    1 = ets:info(T, size),
    [{exists, "new"}] = ets:lookup(T, exists),

    ets:delete(T),
    ok.

%% --- Helpers ---

stress_incr(_T, 0) -> ok;
stress_incr(T, N) ->
    ets:update_counter(T, counter, 1),
    stress_incr(T, N - 1).
