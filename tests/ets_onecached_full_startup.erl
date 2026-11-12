%% SKIP: requires external onecached modules
-module(ets_onecached_full_startup).
-export([test/0]).

%% Replicate the EXACT onecached:start/0 sequence with all modules,
%% and diagnose where the ETS stats disappear.

test() ->
    %% Step 1: sasl (no-op)
    application:start(sasl),

    %% Step 2: init storage (creates tables + inserts counters)
    onecached_storage:init(ets),

    %% Step 3: verify stats exist BEFORE supervisor starts
    check_stats("after init"),

    %% Step 4: start the application (supervisor + children)
    application:start(onecached),

    %% Step 5: verify stats exist AFTER supervisor starts
    check_stats("after app start"),

    %% Step 6: try update_counter from current process
    1 = ets:update_counter(onecached_stats, cmd_set, 1),

    %% Step 7: try from a child process
    Parent = self(),
    spawn(fun() ->
        Result = catch ets:update_counter(onecached_stats, cmd_set, 1),
        Parent ! {uc_result, Result}
    end),
    receive
        {uc_result, 2} -> ok;
        {uc_result, Other} ->
            error_logger:error_msg("Cross-process update_counter failed: ~p~n", [Other]),
            error({cross_process_failed, Other})
    after 5000 ->
        error(timeout)
    end,

    %% Clean exit
    halt(0).

check_stats(Phase) ->
    case catch ets:lookup(onecached_stats, cmd_set) of
        [{cmd_set, V}] ->
            error_logger:info_msg("~s: cmd_set = ~p (OK)~n", [Phase, V]);
        Other ->
            error_logger:error_msg("~s: cmd_set lookup FAILED: ~p~n", [Phase, Other]),
            error({stats_missing, Phase, Other})
    end.
