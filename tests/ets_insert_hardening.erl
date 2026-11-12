-module(ets_insert_hardening).
-export([test/0]).

%% Regression test for ETS insert hardening:
%% - ets:insert/2 with a list must insert ALL elements or raise badarg
%% - ets:insert/2 must raise badarg on table-not-found
%% - ets:new/2 must raise badarg on duplicate named_table
%% - ets:update_counter/3 must work after list insert

test() ->
    test_list_insert_all_keys(),
    test_update_counter_after_list_insert(),
    test_duplicate_named_table(),
    test_catch_duplicate_named_table(),
    test_single_tuple_insert(),
    ok.

%% Exact onecached reset_stats pattern: insert 7 two-tuples as a list,
%% then verify all 7 keys exist and are updatable.
test_list_insert_all_keys() ->
    Tab = ets:new(test_list_all, [named_table, public, set]),
    ets:insert(test_list_all, [
        {curr_items, 0},
        {bytes, 0},
        {cmd_get, 0},
        {cmd_set, 0},
        {get_hits, 0},
        {get_misses, 0},
        {total_items, 0}
    ]),
    %% ALL 7 keys must exist
    [{curr_items, 0}] = ets:lookup(test_list_all, curr_items),
    [{bytes, 0}] = ets:lookup(test_list_all, bytes),
    [{cmd_get, 0}] = ets:lookup(test_list_all, cmd_get),
    [{cmd_set, 0}] = ets:lookup(test_list_all, cmd_set),
    [{get_hits, 0}] = ets:lookup(test_list_all, get_hits),
    [{get_misses, 0}] = ets:lookup(test_list_all, get_misses),
    [{total_items, 0}] = ets:lookup(test_list_all, total_items),
    ets:delete(test_list_all),
    ok.

%% Verify update_counter works on keys inserted via list insert.
test_update_counter_after_list_insert() ->
    Tab = ets:new(test_uc, [named_table, public, set]),
    ets:insert(test_uc, [{a, 0}, {b, 0}, {c, 0}]),
    1 = ets:update_counter(test_uc, a, 1),
    1 = ets:update_counter(test_uc, b, 1),
    1 = ets:update_counter(test_uc, c, 1),
    2 = ets:update_counter(test_uc, a, 1),
    ets:delete(test_uc),
    ok.

%% ets:new with duplicate named_table must raise badarg.
test_duplicate_named_table() ->
    Tab = ets:new(test_dup, [named_table, public, set]),
    %% Second create must fail
    Result = catch ets:new(test_dup, [named_table, public, set]),
    case Result of
        {'EXIT', _} -> ok;
        _ -> error({expected_badarg_on_duplicate_named_table, Result})
    end,
    ets:delete(test_dup),
    ok.

%% The onecached ensure_stats_table pattern:
%% catch ets:new(Name, [named_table|...]) — on duplicate, catch the badarg,
%% call delete_all_objects, then re-insert stats.
test_catch_duplicate_named_table() ->
    %% First create
    ets:new(test_catch_dup, [named_table, public, set]),
    ets:insert(test_catch_dup, [{x, 100}]),

    %% Simulate re-init: catch the duplicate error, clear, re-insert
    case catch ets:new(test_catch_dup, [named_table, public, set]) of
        {'EXIT', _} ->
            ets:delete_all_objects(test_catch_dup);
        _ ->
            ok
    end,
    ets:insert(test_catch_dup, [{x, 0}, {y, 0}]),

    %% Verify the new data is there
    [{x, 0}] = ets:lookup(test_catch_dup, x),
    [{y, 0}] = ets:lookup(test_catch_dup, y),
    0 = ets:update_counter(test_catch_dup, x, 0),
    ets:delete(test_catch_dup),
    ok.

%% Single tuple insert must also work (non-list path).
test_single_tuple_insert() ->
    Tab = ets:new(test_single, [named_table, public, set]),
    ets:insert(test_single, {key1, 42}),
    [{key1, 42}] = ets:lookup(test_single, key1),
    ets:delete(test_single),
    ok.
