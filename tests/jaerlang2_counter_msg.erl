%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (counter2.erl)
%% Tests: spawn, request-reply message passing, PID matching in receive
-module(jaerlang2_counter_msg).
-export([test/0]).

test() ->
    Pid = spawn(fun() -> counter(0) end),

    %% Bump the counter 3 times
    Pid ! bump,
    Pid ! bump,
    Pid ! bump,

    %% Read the counter value
    3 = read(Pid),

    %% Bump once more
    Pid ! bump,
    4 = read(Pid),

    %% Stop the counter
    Pid ! stop,

    ok.

read(Pid) ->
    Pid ! {self(), read},
    receive
        {Pid, N} ->
            N
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
