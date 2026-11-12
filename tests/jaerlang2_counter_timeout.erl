%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (counter3.erl)
%% Tests: spawn, bump/read protocol, receive...after timeout on client side
-module(jaerlang2_counter_timeout).
-export([test/0]).

test() ->
    Pid = spawn(fun() -> counter(0) end),

    %% Bump a few times
    bump(Pid),
    bump(Pid),
    bump(Pid),
    bump(Pid),
    bump(Pid),

    %% Read should return 5
    5 = read(Pid),

    %% Stop the counter and try to read again => timeout (noreply)
    Pid ! stop,
    %% Give the counter process time to handle stop
    sleep(50),
    noreply = read(Pid),

    ok.

bump(Pid) -> Pid ! bump.

read(Pid) ->
    Pid ! {self(), read},
    receive
        {Pid, N} ->
            N
    after 200 ->
        noreply
    end.

counter(N) ->
    receive
        bump ->
            counter(N + 1);
        {From, read} ->
            From ! {self(), N},
            counter(N);
        stop ->
            true
    end.

sleep(T) ->
    receive
    after T -> true
    end.
