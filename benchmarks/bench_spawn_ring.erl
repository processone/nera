%% Benchmark: process ring with computation.
%%
%% Spawns N processes in a ring, sends a token around M times.
%% Each process does a small computation (fibonacci) on each pass
%% to stress both message passing and compute. Inspired by Joe
%% Armstrong's ring benchmark from "Programming Erlang" Ch.8.
%%
%% Usage: time ./target/release/nera run benchmarks/bench_spawn_ring.erl
-module(bench_spawn_ring).
-export([start/0]).

start() ->
    N = 1000,     %% number of processes in ring
    M = 100,      %% messages around the ring
    T0 = erlang:monotonic_time(millisecond),

    %% Build ring: last process sends back to first
    First = self(),
    Last = build_ring(N - 1, First),
    %% Send initial token
    Last ! {token, M, 0},
    %% First process participates in the ring
    Result = ring_loop(Last, M),

    T1 = erlang:monotonic_time(millisecond),
    io:format("bench_spawn_ring: ~w procs, ~w rounds, result=~w~n", [N, M, Result]),
    io:format("time_ms:~w~n", [T1 - T0]),
    ok.

build_ring(0, Next) -> Next;
build_ring(I, Next) ->
    Pid = spawn(fun() -> ring_node(Next) end),
    build_ring(I - 1, Pid).

ring_node(Next) ->
    receive
        {token, 0, Acc} ->
            Next ! {token, 0, Acc};
        {token, Remaining, Acc} ->
            %% Small compute per hop: fib(12) = 144
            F = fib(12),
            Next ! {token, Remaining, Acc + F},
            ring_node(Next)
    end.

ring_loop(_Last, 0) ->
    receive {token, 0, Acc} -> Acc end;
ring_loop(Last, Remaining) ->
    receive
        {token, _, Acc} ->
            F = fib(12),
            Last ! {token, Remaining - 1, Acc + F},
            ring_loop(Last, Remaining - 1)
    end.

fib(0) -> 0;
fib(1) -> 1;
fib(N) -> fib(N - 1) + fib(N - 2).
