-module(fibonacci).
-export([start/0, fib/1]).

%% Non-tail recursive Fibonacci to test stack-based recursion.
%% Also tests comparison operators and arithmetic.

start() ->
    io:format("fib(0) = ~w~n", [fib(0)]),
    io:format("fib(1) = ~w~n", [fib(1)]),
    io:format("fib(5) = ~w~n", [fib(5)]),
    io:format("fib(10) = ~w~n", [fib(10)]),
    io:format("Fibonacci test passed~n").

fib(0) -> 0;
fib(1) -> 1;
fib(N) ->
    fib(N - 1) + fib(N - 2).
