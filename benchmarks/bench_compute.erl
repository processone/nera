%% Benchmark: pure compute — no I/O, no message passing.
%%
%% Exercises tail-recursive loops, pattern matching, list operations,
%% and small-int arithmetic. Inspired by Joe Armstrong's benchmarks
%% and OTP's emulator_bench_SUITE.
%%
%% Usage: time ./target/release/nera run benchmarks/bench_compute.erl
-module(bench_compute).
-export([start/0]).

start() ->
    T0 = erlang:monotonic_time(millisecond),

    %% 1. Recursive fibonacci (exponential, stresses call overhead)
    832040 = fib(30),

    %% 2. Sum of 1..100000 via tail-recursive accumulator
    5000050000 = sum_to(100000),

    %% 3. Tail-recursive length of a 100K list
    L = lists:seq(1, 100000),
    100000 = len(L),

    %% 4. Lists:reverse 10 times on 100K list
    reverse_n(10, L),

    %% 5. Ackermann(3, 7) = 1021 — deeply recursive
    1021 = ack(3, 7),

    %% 6. Sieve of Eratosthenes up to 10000
    Primes = sieve(10000),
    1229 = length(Primes),

    %% 7. Map + filter pipeline on 50K list
    L2 = lists:seq(1, 50000),
    Squares = lists:map(fun(X) -> X * X end, L2),
    Evens = lists:filter(fun(X) -> X rem 2 =:= 0 end, Squares),
    25000 = length(Evens),

    T1 = erlang:monotonic_time(millisecond),
    io:format("bench_compute: all subtasks done~n"),
    io:format("time_ms:~w~n", [T1 - T0]),
    ok.

fib(0) -> 0;
fib(1) -> 1;
fib(N) -> fib(N - 1) + fib(N - 2).

sum_to(N) -> sum_to(N, 0).
sum_to(0, Acc) -> Acc;
sum_to(N, Acc) -> sum_to(N - 1, Acc + N).

len(L) -> len(L, 0).
len([], N) -> N;
len([_|T], N) -> len(T, N + 1).

reverse_n(0, _L) -> ok;
reverse_n(N, L) ->
    _ = lists:reverse(L),
    reverse_n(N - 1, L).

ack(0, N) -> N + 1;
ack(M, 0) -> ack(M - 1, 1);
ack(M, N) -> ack(M - 1, ack(M, N - 1)).

%% Simple sieve using tail-recursive filtering
sieve(Max) ->
    sieve(lists:seq(2, Max), []).

sieve([], Acc) -> lists:reverse(Acc);
sieve([P|Rest], Acc) ->
    Filtered = remove_multiples(P, Rest),
    sieve(Filtered, [P|Acc]).

remove_multiples(P, L) -> remove_multiples(P, L, []).
remove_multiples(_P, [], Acc) -> lists:reverse(Acc);
remove_multiples(P, [H|T], Acc) ->
    case H rem P of
        0 -> remove_multiples(P, T, Acc);
        _ -> remove_multiples(P, T, [H|Acc])
    end.
