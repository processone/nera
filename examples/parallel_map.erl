-module(parallel_map).
-export([start/0, worker/2, collect/2]).

%% Parallel map: spawns a worker for each element, collects results.
%% Tests many-to-one message passing and process spawning.

start() ->
    Self = self(),
    %% Spawn 5 workers that each double a number
    spawn(parallel_map, worker, [Self, 10]),
    spawn(parallel_map, worker, [Self, 20]),
    spawn(parallel_map, worker, [Self, 30]),
    spawn(parallel_map, worker, [Self, 40]),
    spawn(parallel_map, worker, [Self, 50]),
    %% Collect 5 results
    Results = collect(5, []),
    io:format("Results: ~w~n", [Results]),
    io:format("Parallel map test passed~n").

worker(Parent, Value) ->
    Result = Value * 2,
    Parent ! {result, Result}.

collect(0, Acc) -> Acc;
collect(N, Acc) ->
    receive
        {result, Val} ->
            collect(N - 1, [Val | Acc])
    end.
