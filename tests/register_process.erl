-module(register_process).
-export([test/0]).

%% Test: register a spawned process and communicate via name
test() ->
    Self = self(),

    %% Spawn a worker, register it, then communicate via name
    Pid = spawn(fun() -> worker_loop(Self) end),
    register(my_worker, Pid),

    %% Verify whereis
    Pid = whereis(my_worker),

    %% Send via registered name
    my_worker ! {ping, Self},

    %% Receive reply
    receive
        pong -> ok
    after 1000 ->
        exit(timeout)
    end,

    %% Tell worker to stop
    my_worker ! stop,

    %% Give it time to exit, then check name is cleaned up
    timer_sleep(50),
    undefined = whereis(my_worker),

    ok.

worker_loop(Parent) ->
    receive
        {ping, From} ->
            From ! pong,
            worker_loop(Parent);
        stop ->
            ok
    end.

timer_sleep(Ms) ->
    receive
    after Ms -> ok
    end.
