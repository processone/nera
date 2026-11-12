-module(bench_ets_selection).
-export([start/0]).
start() ->
    run(set, 1, 100000),
    run(ordered_set, 1, 100000),
    run(set, 50000, 20),
    run(ordered_set, 50000, 20),
    halt(0).
run(Kind, Size, Repeats) ->
    T = ets:new(bench, [Kind]),
    fill(T, Size),
    Spec = [{{'_', '$1'}, [{'<', '$1', 0}], [true]}],
    Start = erlang:monotonic_time(microsecond),
    ok = scan(T, Spec, Repeats),
    Elapsed = erlang:monotonic_time(microsecond) - Start,
    Size = ets:info(T, size),
    io:format("~p ~p ~p~n", [Kind, Size, Elapsed]),
    true = ets:delete(T).
fill(_, 0) -> ok;
fill(T, N) -> true = ets:insert(T, {N, N}), fill(T, N - 1).
scan(_, _, 0) -> ok;
scan(T, Spec, N) -> 0 = ets:select_delete(T, Spec), scan(T, Spec, N - 1).
