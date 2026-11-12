-module(ets_cross_process_stats).
-export([test/0]).

%% Reproduce multi-process OneCached pattern:
%% Process 1 creates ETS table and inserts stats
%% Process 2 tries to update_counter on those stats
%% This mimics: onecached:start -> init(ets) -> then server processes use stats

test() ->
    %% Process 1 (initial): create table and insert stats
    ets:new(xproc_stats, [named_table, public, set]),
    ets:insert(xproc_stats,
        [{curr_items, 0},
         {bytes, 0},
         {cmd_get, 0},
         {cmd_set, 0},
         {get_hits, 0},
         {get_misses, 0},
         {total_items, 0}]),

    %% Verify from this process first
    [{cmd_set, 0}] = ets:lookup(xproc_stats, cmd_set),

    %% Spawn a child process that tries update_counter
    Parent = self(),
    spawn(fun() ->
        %% This is the pattern from onecached_server:handle_command
        Result = catch ets:update_counter(xproc_stats, cmd_set, 1),
        Parent ! {result, Result}
    end),

    receive
        {result, 1} ->
            ok;
        {result, Other} ->
            error({unexpected_update_counter_result, Other})
    after 5000 ->
        error(timeout)
    end,

    ets:delete(xproc_stats),
    ok.
