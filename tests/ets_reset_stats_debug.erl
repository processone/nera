-module(ets_reset_stats_debug).
-export([test/0]).

%% Reproduce the exact OneCached reset_stats pattern:
%% 1. Create named_table with same options as onecached_stats
%% 2. List-insert 7 counters
%% 3. Verify each key exists via lookup
%% 4. Verify update_counter works on each key

test() ->
    %% Exact onecached_storage:ensure_stats_table pattern
    Tab = ets:new(onecached_stats, [named_table, public, set]),

    %% Exact onecached_storage:reset_stats pattern
    ets:insert(onecached_stats,
        [{curr_items, 0},
         {bytes, 0},
         {cmd_get, 0},
         {cmd_set, 0},
         {get_hits, 0},
         {get_misses, 0},
         {total_items, 0}]),

    %% Verify all 7 keys exist
    [{curr_items, 0}] = ets:lookup(onecached_stats, curr_items),
    [{bytes, 0}] = ets:lookup(onecached_stats, bytes),
    [{cmd_get, 0}] = ets:lookup(onecached_stats, cmd_get),
    [{cmd_set, 0}] = ets:lookup(onecached_stats, cmd_set),
    [{get_hits, 0}] = ets:lookup(onecached_stats, get_hits),
    [{get_misses, 0}] = ets:lookup(onecached_stats, get_misses),
    [{total_items, 0}] = ets:lookup(onecached_stats, total_items),

    %% Verify update_counter works (exact increment_counter pattern)
    1 = ets:update_counter(onecached_stats, cmd_set, 1),
    1 = ets:update_counter(onecached_stats, cmd_get, 1),
    2 = ets:update_counter(onecached_stats, cmd_set, 1),
    1 = ets:update_counter(onecached_stats, get_hits, 1),
    1 = ets:update_counter(onecached_stats, get_misses, 1),

    ets:delete(onecached_stats),
    ok.
