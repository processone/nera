%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (Ch. 13 keep_alive)
%% Tests: monitor/2, DOWN messages, register/2, whereis/1, auto-restart on crash
-module(jaerlang2_keep_alive).
-export([test/0]).

test() ->
    Self = self(),

    %% Start a counter server that auto-restarts on crash
    keep_alive(my_counter, fun() -> counter_loop(0) end),
    sleep(50),

    %% Verify it's registered
    Pid1 = whereis(my_counter),
    true = is_pid(Pid1),

    %% Use the server: increment and get
    my_counter ! {Self, increment},
    ok = receive R1 -> R1 after 1000 -> timeout end,
    my_counter ! {Self, increment},
    ok = receive R2 -> R2 after 1000 -> timeout end,
    my_counter ! {Self, get},
    {count, 2} = receive R3 -> R3 after 1000 -> timeout end,

    %% Tell the server to crash
    my_counter ! crash,
    sleep(200),

    %% Verify it restarted with a new PID
    Pid2 = whereis(my_counter),
    true = is_pid(Pid2),
    true = (Pid1 =/= Pid2),

    %% Verify the new server works (state was reset to 0)
    my_counter ! {Self, get},
    {count, 0} = receive R4 -> R4 after 1000 -> timeout end,

    %% Increment the new server
    my_counter ! {Self, increment},
    ok = receive R5 -> R5 after 1000 -> timeout end,
    my_counter ! {Self, get},
    {count, 1} = receive R6 -> R6 after 1000 -> timeout end,

    %% Clean up: stop the restarter by stopping the server gracefully
    my_counter ! stop,
    sleep(50),
    ok.

%% keep_alive(Name, Fun) - spawn Fun registered as Name,
%% auto-restart on crash using a monitor process.
keep_alive(Name, Fun) ->
    Pid = spawn(Fun),
    register(Name, Pid),
    %% Watcher process that monitors and restarts on non-normal exit
    spawn(fun() -> watcher(Name, Fun, Pid) end).

watcher(Name, Fun, Pid) ->
    Ref = monitor(process, Pid),
    receive
        {'DOWN', Ref, process, Pid, normal} ->
            %% Normal exit, don't restart
            ok;
        {'DOWN', Ref, process, Pid, _Reason} ->
            %% Abnormal exit, restart
            sleep(50),
            keep_alive(Name, Fun)
    end.

%% Simple counter server
counter_loop(N) ->
    receive
        {From, increment} ->
            From ! ok,
            counter_loop(N + 1);
        {From, get} ->
            From ! {count, N},
            counter_loop(N);
        crash ->
            exit(crashed);
        stop ->
            ok
    end.

sleep(T) ->
    receive after T -> ok end.
