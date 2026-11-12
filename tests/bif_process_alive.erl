%% Tests is_process_alive/1 BIF.
-module(bif_process_alive).
-export([test/0]).

test() ->
    %% Self is alive
    true = is_process_alive(self()),

    %% Spawn a process that exits quickly
    Parent = self(),
    Pid = spawn(fun() ->
        Parent ! {self(), started},
        ok
    end),
    %% Wait for it to start (and it will exit right after sending)
    receive {Pid, started} -> ok end,
    %% Give it time to fully exit
    receive after 50 -> ok end,
    false = is_process_alive(Pid),

    %% Spawn a process that stays alive
    Pid2 = spawn(fun() ->
        receive stop -> ok end
    end),
    receive after 10 -> ok end,
    true = is_process_alive(Pid2),

    %% Tell it to stop
    Pid2 ! stop,
    receive after 50 -> ok end,
    false = is_process_alive(Pid2),

    io:format("ok~n"),
    ok.
