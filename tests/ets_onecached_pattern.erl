%% SKIP: requires external onecached_storage module
-module(ets_onecached_pattern).
-export([test/0]).

%% Use the ACTUAL onecached_storage module to test init + update_counter.
%% This replicates what onecached:start/0 does, then what handle_command does.

test() ->
    %% Step 1: init (creates tables, inserts stats)
    onecached_storage:init(ets),

    %% Step 2: verify the stats table exists and has keys
    [{cmd_set, 0}] = ets:lookup(onecached_stats, cmd_set),
    [{cmd_get, 0}] = ets:lookup(onecached_stats, cmd_get),

    %% Step 3: increment (what handle_command does)
    onecached_storage:increment_cmd_set(ets),
    onecached_storage:increment_cmd_get(ets),
    onecached_storage:increment_cmd_set(ets),

    %% Step 4: verify stats
    [{cmd_set, Val1}] = ets:lookup(onecached_stats, cmd_set),
    2 = Val1,
    [{cmd_get, Val2}] = ets:lookup(onecached_stats, cmd_get),
    1 = Val2,

    ets:delete(onecached_stats),
    ets:delete(onecached),
    ok.
