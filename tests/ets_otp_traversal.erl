%% Tests ported from OTP ets_SUITE.erl: first/next traversal, tab2list.
%% Adapted to nera test convention (test/0 -> ok, badmatch = failure).
-module(ets_otp_traversal).
-export([test/0]).

test() ->
    test_firstnext_complete(),
    test_firstnext_single(),
    test_tab2list_order_independent(),
    test_tab2list_after_mutations(),
    test_iterate_and_delete(),
    test_firstnext_large(),
    ok.

%% OTP firstnext_do: complete traversal visiting all keys exactly once
test_firstnext_complete() ->
    T = ets:new(t, [set, public]),
    N = 50,
    fill_tab(T, 1, N),

    %% Collect all keys via first/next
    Keys = collect_keys(T, ets:first(T), []),
    N = length(Keys),

    %% Every key from 1..N must appear exactly once
    Sorted = lists:sort(Keys),
    Expected = lists:seq(1, N),
    Expected = Sorted,

    ets:delete(T),
    ok.

%% Single entry: first returns key, next returns '$end_of_table'
test_firstnext_single() ->
    T = ets:new(t, [set, public]),
    ets:insert(T, {only, 42}),

    only = ets:first(T),
    '$end_of_table' = ets:next(T, only),

    ets:delete(T),
    ok.

%% OTP: tab2list returns all entries, order-independent verification
test_tab2list_order_independent() ->
    T = ets:new(t, [set, public]),

    Entries = [{a, 1}, {b, 2}, {c, 3}, {d, 4}, {e, 5}],
    ets:insert(T, Entries),

    List = ets:tab2list(T),
    5 = length(List),

    %% All entries present (order may differ from insertion order)
    true = lists:member({a, 1}, List),
    true = lists:member({b, 2}, List),
    true = lists:member({c, 3}, List),
    true = lists:member({d, 4}, List),
    true = lists:member({e, 5}, List),

    ets:delete(T),
    ok.

%% tab2list reflects mutations
test_tab2list_after_mutations() ->
    T = ets:new(t, [set, public]),
    fill_tab(T, 1, 10),
    10 = length(ets:tab2list(T)),

    %% Delete some
    ets:delete(T, 3),
    ets:delete(T, 7),
    8 = length(ets:tab2list(T)),

    %% Overwrite
    ets:insert(T, {1, 999}),
    List = ets:tab2list(T),
    8 = length(List),
    true = lists:member({1, 999}, List),
    false = lists:member({1, 10}, List),

    %% Add new
    ets:insert(T, {100, 1000}),
    9 = length(ets:tab2list(T)),

    %% Clear
    ets:delete_all_objects(T),
    [] = ets:tab2list(T),

    ets:delete(T),
    ok.

%% Iterate with first/next, deleting entries during traversal
%% (This tests safe iteration — BEAM allows deleting during traversal)
test_iterate_and_delete() ->
    T = ets:new(t, [set, public]),
    ets:insert(T, [{a, 1}, {b, 2}, {c, 3}, {d, 4}, {e, 5}]),

    %% Delete every entry we visit during traversal
    delete_during_traverse(T, ets:first(T)),

    %% All entries should be deleted
    0 = ets:info(T, size),
    [] = ets:tab2list(T),

    ets:delete(T),
    ok.

%% Larger traversal: 500 entries
test_firstnext_large() ->
    T = ets:new(t, [set, public]),
    N = 500,
    fill_tab(T, 1, N),

    %% Count via traversal matches info size
    Count = count_keys(T, ets:first(T), 0),
    N = Count,
    N = ets:info(T, size),

    %% Sum all keys via traversal
    Sum = sum_keys(T, ets:first(T), 0),
    %% Sum 1..500 = 500*501/2 = 125250
    125250 = Sum,

    ets:delete(T),
    ok.

%% --- Helpers ---

fill_tab(_T, I, Max) when I > Max -> ok;
fill_tab(T, I, Max) ->
    ets:insert(T, {I, I * 10}),
    fill_tab(T, I + 1, Max).

collect_keys(_T, '$end_of_table', Acc) -> Acc;
collect_keys(T, Key, Acc) ->
    collect_keys(T, ets:next(T, Key), [Key | Acc]).

count_keys(_T, '$end_of_table', Acc) -> Acc;
count_keys(T, Key, Acc) ->
    count_keys(T, ets:next(T, Key), Acc + 1).

sum_keys(_T, '$end_of_table', Acc) -> Acc;
sum_keys(T, Key, Acc) ->
    sum_keys(T, ets:next(T, Key), Acc + Key).

delete_during_traverse(_T, '$end_of_table') -> ok;
delete_during_traverse(T, Key) ->
    Next = ets:next(T, Key),
    ets:delete(T, Key),
    delete_during_traverse(T, Next).
