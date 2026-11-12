-module(bench_ets_counters).
-export([start/0]).

start() ->
    run(set),
    run(ordered_set),
    halt(0).

run(Kind) ->
    T = ets:new(bench, [Kind]),
    ets:insert(T, {key, 0}),
    measure(Kind, single, T, 1),
    measure(Kind, batch, T, [{2, 1}, {2, -1}, {2, 1}]),
    [{key, 2000000}] = ets:lookup(T, key),
    ets:delete(T).

measure(Kind, Phase, T, Op) ->
    T0 = erlang:monotonic_time(microsecond),
    loop(T, Op, 1000000),
    T1 = erlang:monotonic_time(microsecond),
    io:format("~p ~p ~p~n", [Kind, Phase, T1 - T0]).

loop(_, _, 0) -> ok;
loop(T, Op, N) ->
    ets:update_counter(T, key, Op),
    loop(T, Op, N - 1).
