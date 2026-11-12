-module(counter).
-export([start/0, loop/1]).

%% A stateful counter process: accepts increment, get, and stop messages.
%% Tests recursive state management across multiple message exchanges.

start() ->
    Counter = spawn(counter, loop, [0]),
    Self = self(),

    Counter ! {increment, 5},
    Counter ! {increment, 3},
    Counter ! {increment, 2},

    Counter ! {get, Self},
    receive
        {value, Val} ->
            io:format("Counter value: ~w~n", [Val])
    end,

    Counter ! {increment, 10},

    Counter ! {get, Self},
    receive
        {value, Val2} ->
            io:format("Counter value after +10: ~w~n", [Val2])
    end,

    Counter ! stop,
    io:format("Counter test passed~n").

loop(Count) ->
    receive
        {increment, N} ->
            loop(Count + N);
        {get, From} ->
            From ! {value, Count},
            loop(Count);
        stop ->
            ok
    end.
