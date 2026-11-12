-module(bench_mul).
-export([main/0, multiply/2]).
multiply(A, B) -> A * B.
loop(0, Acc) -> Acc;
loop(N, Acc) ->
    Product = multiply((N band 1023) - 512, (N band 15) - 8),
    loop(N - 1, Acc + Product).
main() ->
    Start = erlang:monotonic_time(microsecond),
    Result = loop(10000000, 0),
    Elapsed = erlang:monotonic_time(microsecond) - Start,
    io:format("~p ~p~n", [Elapsed, Result]).
