%% Adapted from OTP compiler test suites (Apache 2.0)
%% Tests recursive functions, including tail recursion.
-module(recursion).
-export([test/0]).

test() ->
    %% Simple recursion (factorial)
    1 = fac(0),
    1 = fac(1),
    6 = fac(3),
    120 = fac(5),
    3628800 = fac(10),

    %% Tail-recursive accumulator style
    3628800 = fac_tail(10),
    1 = fac_tail(0),

    %% Fibonacci
    0 = fib(0),
    1 = fib(1),
    1 = fib(2),
    5 = fib(5),
    55 = fib(10),

    %% List length (tail recursive)
    0 = len([]),
    1 = len([a]),
    5 = len([1, 2, 3, 4, 5]),

    %% List reverse (tail recursive)
    [] = rev([]),
    [1] = rev([1]),
    [3, 2, 1] = rev([1, 2, 3]),

    %% Sum of list
    0 = sum([]),
    15 = sum([1, 2, 3, 4, 5]),

    %% Map over list (body recursive)
    [] = double([]),
    [2, 4, 6] = double([1, 2, 3]),

    %% Filter
    [2, 4] = filter_even([1, 2, 3, 4, 5]),

    %% Ackermann (deeply recursive)
    3 = ack(1, 1),
    7 = ack(2, 2),

    io:format("ok~n"),
    ok.

fac(0) -> 1;
fac(N) when N > 0 -> N * fac(N - 1).

fac_tail(N) -> fac_tail(N, 1).
fac_tail(0, Acc) -> Acc;
fac_tail(N, Acc) when N > 0 -> fac_tail(N - 1, N * Acc).

fib(0) -> 0;
fib(1) -> 1;
fib(N) when N > 1 -> fib(N - 1) + fib(N - 2).

len(L) -> len(L, 0).
len([], Acc) -> Acc;
len([_ | T], Acc) -> len(T, Acc + 1).

rev(L) -> rev(L, []).
rev([], Acc) -> Acc;
rev([H | T], Acc) -> rev(T, [H | Acc]).

sum(L) -> sum(L, 0).
sum([], Acc) -> Acc;
sum([H | T], Acc) -> sum(T, Acc + H).

double([]) -> [];
double([H | T]) -> [H * 2 | double(T)].

filter_even([]) -> [];
filter_even([H | T]) when H rem 2 == 0 -> [H | filter_even(T)];
filter_even([_ | T]) -> filter_even(T).

ack(0, N) -> N + 1;
ack(M, 0) when M > 0 -> ack(M - 1, 1);
ack(M, N) when M > 0, N > 0 -> ack(M - 1, ack(M, N - 1)).
