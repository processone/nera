%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (cookbook_examples.erl)
%% Tests: recursive fibonacci (naive + tail-recursive), arithmetic
-module(jaerlang2_fib).
-export([test/0]).

test() ->
    %% Naive recursive fibonacci
    1  = fib1(0),
    1  = fib1(1),
    2  = fib1(2),
    3  = fib1(3),
    5  = fib1(4),
    8  = fib1(5),
    89 = fib1(10),

    %% Tail-recursive fibonacci
    1  = fib2(0),
    1  = fib2(1),
    2  = fib2(2),
    3  = fib2(3),
    5  = fib2(4),
    8  = fib2(5),
    89 = fib2(10),

    %% Both should agree
    true = (fib1(15) =:= fib2(15)),

    ok.

%% Naive recursive
fib1(0) -> 1;
fib1(1) -> 1;
fib1(N) -> fib1(N-1) + fib1(N-2).

%% Tail-recursive
fib2(N) -> fib2(N, 1, 1).
fib2(0, A, _) -> A;
fib2(N, A, B) -> fib2(N-1, B, A+B).
