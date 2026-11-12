%% Benchmark: pure tail-recursive loops — tests reduction-counter overhead.
%%
%% Exercises self-tail-call loops with minimal body work (just arithmetic).
%% Primary target for register-based reduction counter optimization:
%% the hot loop should use only CPU registers, no memory load/store
%% for the reduction counter.
%%
%% Usage: time ./target/release/nera build benchmarks/bench_tailcall.erl -o /tmp/bench_tailcall && /tmp/bench_tailcall
-module(bench_tailcall).
-export([start/0]).

start() ->
    T0 = erlang:monotonic_time(millisecond),

    %% 1. Sum 1..1_000_000 ten times (pure self-tail-call + small-int arith)
    run_sum(10),

    %% 2. Count down from 10_000_000 (minimal body, just decrement)
    countdown(10000000),

    %% 3. Length of a 500K list (tail-recursive, cons cell traversal)
    L = lists:seq(1, 500000),
    500000 = len(L),

    T1 = erlang:monotonic_time(millisecond),
    io:format("bench_tailcall: all subtasks done~n"),
    io:format("time_ms:~w~n", [T1 - T0]),
    ok.

run_sum(0) -> ok;
run_sum(N) ->
    500000500000 = sum_to(1000000),
    run_sum(N - 1).

sum_to(N) -> sum_to(N, 0).
sum_to(0, Acc) -> Acc;
sum_to(N, Acc) -> sum_to(N - 1, Acc + N).

countdown(0) -> ok;
countdown(N) -> countdown(N - 1).

len(L) -> len(L, 0).
len([], N) -> N;
len([_|T], N) -> len(T, N + 1).
