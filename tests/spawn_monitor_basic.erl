%% Tests for spawn_monitor/1 and spawn_monitor/3.
%% Verifies atomic spawn+monitor, DOWN message delivery, and noproc semantics.
-module(spawn_monitor_basic).
-export([test/0, worker_fun/1, crasher/1]).

test() ->
    test_closure_normal_exit(),
    test_closure_crash_exit(),
    test_mfa_normal_exit(),
    test_mfa_crash_exit(),
    test_ref_is_reference(),
    test_demonitor_cancels(),
    test_multiple_spawn_monitors(),
    ok.

%% --- Test 1: spawn_monitor/1, child exits normally ---
test_closure_normal_exit() ->
    Self = self(),
    {Pid, Ref} = spawn_monitor(fun() -> Self ! {hello, self()} end),

    %% Should get the message from child
    {hello, Pid} = receive Msg1 -> Msg1 after 2000 -> exit(no_hello) end,

    %% Should get DOWN with reason 'normal'
    receive
        {'DOWN', Ref, process, Pid, normal} -> ok
    after 2000 ->
        exit(no_down_normal)
    end,
    ok.

%% --- Test 2: spawn_monitor/1, child exits with error ---
test_closure_crash_exit() ->
    {Pid, Ref} = spawn_monitor(fun() -> exit(boom) end),

    %% Should get DOWN with reason 'boom'
    receive
        {'DOWN', Ref, process, Pid, boom} -> ok
    after 2000 ->
        exit(no_down_boom)
    end,
    ok.

%% --- Test 3: spawn_monitor/3, child exits normally ---
test_mfa_normal_exit() ->
    Self = self(),
    {Pid, Ref} = spawn_monitor(?MODULE, worker_fun, [Self]),

    %% Worker sends {working, Pid}
    {working, Pid} = receive Msg2 -> Msg2 after 2000 -> exit(no_working) end,

    %% Should get DOWN normal
    receive
        {'DOWN', Ref, process, Pid, normal} -> ok
    after 2000 ->
        exit(no_down_mfa)
    end,
    ok.

%% --- Test 4: spawn_monitor/3 with crashing function ---
test_mfa_crash_exit() ->
    {Pid, Ref} = spawn_monitor(?MODULE, crasher, [kaboom]),

    receive
        {'DOWN', Ref, process, Pid, kaboom} -> ok
    after 2000 ->
        exit(no_down_mfa_crash)
    end,
    ok.

%% --- Test 5: Ref returned is a proper reference ---
test_ref_is_reference() ->
    {_Pid, Ref} = spawn_monitor(fun() -> ok end),
    true = is_reference(Ref),
    %% Clean up DOWN message
    receive {'DOWN', Ref, process, _, _} -> ok after 2000 -> ok end,
    ok.

%% --- Test 6: demonitor cancels, no DOWN received ---
test_demonitor_cancels() ->
    Self = self(),
    {_Pid, Ref} = spawn_monitor(fun() ->
                                    %% Wait for parent to demonitor before exiting
                                    receive go -> ok after 2000 -> ok end
                                end),
    demonitor(Ref),
    Self ! {_Pid, go},
    %% Extract Pid from the tuple we sent ourselves
    {ChildPid, go} = receive DMsg -> DMsg after 1000 -> exit(no_go) end,
    ChildPid ! go,
    %% Should NOT receive a DOWN message
    receive
        {'DOWN', Ref, process, _, _} -> exit(unexpected_down)
    after 300 ->
        ok
    end,
    ok.

%% --- Test 7: Multiple spawn_monitors, each gets its own DOWN ---
test_multiple_spawn_monitors() ->
    {P1, R1} = spawn_monitor(fun() -> exit(reason1) end),
    {P2, R2} = spawn_monitor(fun() -> exit(reason2) end),
    {P3, R3} = spawn_monitor(fun() -> exit(reason3) end),

    %% Collect all three DOWNs (order may vary)
    ok = collect_downs([{P1, R1, reason1}, {P2, R2, reason2}, {P3, R3, reason3}], 3),
    ok.

collect_downs(_, 0) -> ok;
collect_downs(Expected, N) ->
    receive
        {'DOWN', Ref, process, Pid, Reason} ->
            true = lists:member({Pid, Ref, Reason}, Expected),
            collect_downs(Expected, N - 1)
    after 2000 ->
        exit({missing_downs, N})
    end.

%% Exported worker function for spawn_monitor/3 test
worker_fun(Parent) ->
    Parent ! {working, self()},
    ok.

crasher(Reason) ->
    exit(Reason).
