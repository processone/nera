%% Tests: exit/2 signal processing, link chains, selective trapping.
-module(link_cascade).
-export([test/0, wait_forever/0, wait_and_reply/1]).

test() ->
    %% Test 1: exit(Pid, kill) on a linked process — parent gets {'EXIT', Pid, killed}
    ok = kill_linked_test(),

    %% Test 2: trap_exit + exit(Pid, kill) → process receives {'EXIT', Pid, killed}
    ok = trap_kill_test(),

    %% Test 3: exit(Pid, Reason) with non-normal reason, no trap_exit → target dies
    ok = exit_reason_test(),

    %% Test 4: exit(Pid, normal) with no trap_exit → ignored (process stays alive)
    ok = exit_normal_ignored_test(),

    %% Test 5: on_exit restart pattern (uses monitor, not links)
    ok = on_exit_test(),

    %% Test 6: Normal exit through links: trapped as {'EXIT', Pid, normal}
    ok = normal_exit_test(),

    io:format("ok~n"),
    halt(0).

%% Helper: block forever in receive
wait_forever() ->
    receive
        never_matches -> ok
    after 60000 ->
        ok
    end.

%% Helper: wait for ping, reply pong
wait_and_reply(From) ->
    receive
        ping -> From ! pong
    after 5000 ->
        ok
    end.

%% Kill a linked process with exit/2, parent (trap_exit) gets {'EXIT', Pid, killed}
kill_linked_test() ->
    process_flag(trap_exit, true),
    Pid = spawn_link(fun wait_forever/0),
    %% Give Worker time to enter receive
    receive after 50 -> ok end,
    exit(Pid, kill),
    receive
        {'EXIT', Pid, killed} -> ok
    after 2000 ->
        error(no_exit_kill)
    end,
    process_flag(trap_exit, false),
    ok.

%% Same test but verifying the message format
trap_kill_test() ->
    process_flag(trap_exit, true),
    Pid = spawn_link(fun wait_forever/0),
    receive after 50 -> ok end,
    exit(Pid, kill),
    receive
        {'EXIT', Pid, killed} -> ok
    after 2000 ->
        error(trap_not_received)
    end,
    process_flag(trap_exit, false),
    ok.

%% exit(Pid, some_reason) with no trap_exit kills the target
exit_reason_test() ->
    process_flag(trap_exit, true),
    Pid = spawn_link(fun wait_forever/0),
    receive after 50 -> ok end,
    exit(Pid, crash_reason),
    receive
        {'EXIT', Pid, crash_reason} -> ok
    after 2000 ->
        error(no_exit_reason)
    end,
    process_flag(trap_exit, false),
    ok.

%% exit(Pid, normal) with no trap_exit is ignored — process stays alive
exit_normal_ignored_test() ->
    Self = self(),
    Pid = spawn(fun() -> wait_and_reply(Self) end),
    receive after 50 -> ok end,
    exit(Pid, normal),
    %% Small delay to let the signal be processed
    receive after 100 -> ok end,
    Pid ! ping,
    receive
        pong -> ok
    after 2000 ->
        error(process_died_from_normal)
    end,
    ok.

%% on_exit: monitor a process, restart when it dies
on_exit_test() ->
    Self = self(),
    Pid1 = spawn(fun worker/0),
    monitor(process, Pid1),
    Pid1 ! crash,
    receive
        {'DOWN', _, process, Pid1, _} -> ok
    after 1000 ->
        error(worker_not_crashed)
    end,
    %% Restart
    Pid2 = spawn(fun worker/0),
    Pid2 ! {ping, Self},
    receive
        pong -> ok
    after 1000 ->
        error(restart_failed)
    end,
    exit(Pid2, kill),
    ok.

%% Normal exit through links: trapped as {'EXIT', Pid, normal}
normal_exit_test() ->
    process_flag(trap_exit, true),
    A = spawn_link(fun() -> ok end),
    receive
        {'EXIT', A, normal} -> ok
    after 2000 ->
        error(no_normal_exit)
    end,
    process_flag(trap_exit, false),
    ok.

worker() ->
    receive
        crash -> error(intentional);
        {ping, From} -> From ! pong
    end.
