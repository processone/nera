%% Adapted from "Programming Erlang" by Joe Armstrong — counter pattern.
%% Tests stateful process with bump/read message protocol.
-module(jaerlang_counter).
-export([test/0]).

id(I) -> I.

test() ->
    Pid = spawn(fun() -> counter(0) end),
    %% Bump 5 times
    Pid ! bump,
    Pid ! bump,
    Pid ! bump,
    Pid ! bump,
    Pid ! bump,
    %% Small delay to let messages process
    sleep(id(50)),
    %% Read the counter
    5 = read(Pid),
    %% Bump 3 more times
    Pid ! bump,
    Pid ! bump,
    Pid ! bump,
    sleep(id(50)),
    8 = read(Pid),
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
            ok
    end.

sleep(T) ->
    receive
    after T -> ok
    end.
