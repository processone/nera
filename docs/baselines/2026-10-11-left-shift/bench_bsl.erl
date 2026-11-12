-module(bench_bsl).
-export([main/0, shift/2]).
shift(A, B) -> A bsl B.
loop(0, Acc) -> Acc;
loop(N, Acc) ->
    Value = shift(N band 1023, N band 7),
    loop(N - 1, Acc + Value).
main() ->
    Start = erlang:monotonic_time(microsecond),
    Result = loop(10000000, 0),
    Elapsed = erlang:monotonic_time(microsecond) - Start,
    io:format("~p ~p~n", [Elapsed, Result]).
