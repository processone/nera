-module(bench_promoted_counters).
-export([start/0]).
start() -> run(set), run(ordered_set), halt(0).
run(Kind) ->
    T = ets:new(bench, [Kind]),
    Start = 4611686018427387904,
    ets:insert(T, {key, Start}),
    T0 = erlang:monotonic_time(microsecond),
    loop(T, 200000),
    T1 = erlang:monotonic_time(microsecond),
    [{key, 4611686018427587904}] = ets:lookup(T, key),
    io:format("~p ~p~n", [Kind, T1 - T0]),
    ets:delete(T).
loop(_, 0) -> ok;
loop(T, N) -> ets:update_counter(T, key, 1), loop(T, N - 1).
