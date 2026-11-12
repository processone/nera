%% Tests ported from OTP ets_SUITE.erl: insert/insert_new/overwrite semantics.
%% Adapted to nera test convention (test/0 -> ok, badmatch = failure).
-module(ets_otp_insert).
-export([test/0]).

test() ->
    test_insert_overwrite(),
    test_insert_new_single(),
    test_insert_new_list_atomicity(),
    test_insert_new_roundtrip(),
    test_insert_batch(),
    ok.

%% OTP: insert on set replaces entire tuple, size stays constant
test_insert_overwrite() ->
    T = ets:new(t, [set, public]),

    ets:insert(T, {key, "old", extra}),
    1 = ets:info(T, size),
    [{key, "old", extra}] = ets:lookup(T, key),

    %% Overwrite — same key, different value and arity
    ets:insert(T, {key, "new"}),
    1 = ets:info(T, size),
    [{key, "new"}] = ets:lookup(T, key),

    %% Overwrite again
    ets:insert(T, {key, 42, a, b, c}),
    1 = ets:info(T, size),
    [{key, 42, a, b, c}] = ets:lookup(T, key),

    %% Insert different key — size grows
    ets:insert(T, {key2, val2}),
    2 = ets:info(T, size),

    ets:delete(T),
    ok.

%% OTP t_insert_new: single-key insert_new semantics
test_insert_new_single() ->
    T = ets:new(t, [set, public]),

    %% First insert_new succeeds
    true = ets:insert_new(T, {1, "one"}),
    [{1, "one"}] = ets:lookup(T, 1),

    %% Second insert_new with same key fails
    false = ets:insert_new(T, {1, "ONE"}),
    %% Original preserved
    [{1, "one"}] = ets:lookup(T, 1),
    1 = ets:info(T, size),

    %% Regular insert always succeeds (overwrites)
    true = ets:insert(T, {1, "ONE"}),
    [{1, "ONE"}] = ets:lookup(T, 1),
    1 = ets:info(T, size),

    %% insert_new with new key succeeds
    true = ets:insert_new(T, {2, "two"}),
    2 = ets:info(T, size),

    ets:delete(T),
    ok.

%% OTP t_insert_new: list form — all-or-nothing atomicity
test_insert_new_list_atomicity() ->
    T = ets:new(t, [set, public]),

    %% Seed one entry
    ets:insert(T, {100, "hundred"}),

    %% List where one key (100) already exists — entire batch rejected
    false = ets:insert_new(T, [{99, "a"}, {100, "b"}, {101, "c"}]),
    1 = ets:info(T, size),
    %% Original preserved, others not inserted
    [{100, "hundred"}] = ets:lookup(T, 100),
    [] = ets:lookup(T, 99),
    [] = ets:lookup(T, 101),

    %% List where all keys are new — succeeds
    true = ets:insert_new(T, [{200, "x"}, {201, "y"}, {202, "z"}]),
    4 = ets:info(T, size),
    [{200, "x"}] = ets:lookup(T, 200),
    [{201, "y"}] = ets:lookup(T, 201),
    [{202, "z"}] = ets:lookup(T, 202),

    %% Now 200 exists — batch with 200 fails
    false = ets:insert_new(T, [{200, "new"}, {300, "new"}]),
    4 = ets:info(T, size),
    [] = ets:lookup(T, 300),

    %% Single-element list
    true = ets:insert_new(T, [{500, "five"}]),
    false = ets:insert_new(T, [{500, "five2"}]),
    [{500, "five"}] = ets:lookup(T, 500),

    ets:delete(T),
    ok.

%% OTP t_insert_new: tab2list → delete_all → insert_new round-trip
test_insert_new_roundtrip() ->
    T = ets:new(t, [set, public]),

    %% Fill table
    fill_tab(T, 1, 50),
    50 = ets:info(T, size),

    %% Extract all, clear, re-insert via insert_new
    List = ets:tab2list(T),
    50 = length(List),
    ets:delete_all_objects(T),
    0 = ets:info(T, size),

    true = ets:insert_new(T, List),
    50 = ets:info(T, size),

    %% Second insert_new with same data fails
    false = ets:insert_new(T, List),
    50 = ets:info(T, size),

    %% Verify all entries present
    verify_entries(T, 1, 50),

    ets:delete(T),
    ok.

%% OTP: batch insert with list of tuples
test_insert_batch() ->
    T = ets:new(t, [set, public]),

    %% Insert list of tuples
    true = ets:insert(T, [{a, 1}, {b, 2}, {c, 3}, {d, 4}, {e, 5}]),
    5 = ets:info(T, size),

    %% All present
    [{a, 1}] = ets:lookup(T, a),
    [{e, 5}] = ets:lookup(T, e),

    %% Batch overwrite — some new, some existing
    true = ets:insert(T, [{a, 10}, {f, 6}, {g, 7}]),
    7 = ets:info(T, size),
    [{a, 10}] = ets:lookup(T, a),
    [{f, 6}] = ets:lookup(T, f),

    ets:delete(T),
    ok.

%% --- Helpers ---

fill_tab(_T, I, Max) when I > Max -> ok;
fill_tab(T, I, Max) ->
    ets:insert(T, {I, I * 10}),
    fill_tab(T, I + 1, Max).

verify_entries(_T, I, Max) when I > Max -> ok;
verify_entries(T, I, Max) ->
    [{I, Val}] = ets:lookup(T, I),
    Val = I * 10,
    verify_entries(T, I + 1, Max).
