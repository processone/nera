-module(factorial).
-export([start/0]).

start() ->
    Result = fac(10),
    io:format("10! = ~w~n", [Result]).

fac(0) -> 1;
fac(N) when N > 0 -> N * fac(N - 1).
