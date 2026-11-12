-module(bench_atomics).
-export([start/0]).

%% atomics microbenchmark — measures put/get, concurrent add,
%% CAS loop, add_get, and exchange throughput.
%% Compatible with both nera and BEAM.

-define(N, 100000).

start() ->
    Ref = atomics:new(4, [{signed, true}]),

    %% 1. Sequential put/get pairs
    T0_putget = erlang:monotonic_time(millisecond),
    put_get_n(Ref, 1, ?N),
    T1_putget = erlang:monotonic_time(millisecond),

    %% 2. Concurrent add: 4 processes × N/4 atomic adds
    atomics:put(Ref, 2, 0),
    T0_conc = erlang:monotonic_time(millisecond),
    concurrent_add(Ref, 2, 4, ?N div 4),
    T1_conc = erlang:monotonic_time(millisecond),
    ConcVal = atomics:get(Ref, 2),

    %% 3. CAS loop: N compare_exchange operations
    atomics:put(Ref, 3, 0),
    T0_cas = erlang:monotonic_time(millisecond),
    cas_loop(Ref, 3, ?N),
    T1_cas = erlang:monotonic_time(millisecond),
    CasVal = atomics:get(Ref, 3),

    %% 4. add_get throughput
    atomics:put(Ref, 4, 0),
    T0_addget = erlang:monotonic_time(millisecond),
    add_get_n(Ref, 4, ?N),
    T1_addget = erlang:monotonic_time(millisecond),

    %% 5. exchange throughput
    atomics:put(Ref, 1, 0),
    T0_xchg = erlang:monotonic_time(millisecond),
    exchange_n(Ref, 1, ?N),
    T1_xchg = erlang:monotonic_time(millisecond),

    %% Report
    io:format("atomics Benchmark (~p ops)~n", [?N]),
    io:format("  put/get:        ~p ms~n", [T1_putget - T0_putget]),
    io:format("  conc_add:       ~p ms  (4 procs, result=~p)~n", [T1_conc - T0_conc, ConcVal]),
    io:format("  CAS loop:       ~p ms  (result=~p)~n", [T1_cas - T0_cas, CasVal]),
    io:format("  add_get:        ~p ms~n", [T1_addget - T0_addget]),
    io:format("  exchange:       ~p ms~n", [T1_xchg - T0_xchg]),
    halt(0).

put_get_n(_, _, 0) -> ok;
put_get_n(Ref, Ix, N) ->
    atomics:put(Ref, Ix, N),
    N = atomics:get(Ref, Ix),
    put_get_n(Ref, Ix, N - 1).

add_get_n(_, _, 0) -> ok;
add_get_n(Ref, Ix, N) ->
    _ = atomics:add_get(Ref, Ix, 1),
    add_get_n(Ref, Ix, N - 1).

exchange_n(_, _, 0) -> ok;
exchange_n(Ref, Ix, N) ->
    _ = atomics:exchange(Ref, Ix, N),
    exchange_n(Ref, Ix, N - 1).

%% Sequential CAS loop: increment by 1 using compare_exchange
cas_loop(_, _, 0) -> ok;
cas_loop(Ref, Ix, N) ->
    cas_increment(Ref, Ix),
    cas_loop(Ref, Ix, N - 1).

cas_increment(Ref, Ix) ->
    Old = atomics:get(Ref, Ix),
    case atomics:compare_exchange(Ref, Ix, Old, Old + 1) of
        ok -> ok;
        _Actual -> cas_increment(Ref, Ix)  % retry
    end.

concurrent_add(Ref, Ix, NumProcs, AddsPerProc) ->
    Parent = self(),
    Pids = [spawn(fun() ->
        add_n(Ref, Ix, AddsPerProc),
        Parent ! {done, self()}
    end) || _ <- lists:seq(1, NumProcs)],
    wait_all(Pids).

add_n(_, _, 0) -> ok;
add_n(Ref, Ix, N) ->
    atomics:add(Ref, Ix, 1),
    add_n(Ref, Ix, N - 1).

wait_all([]) -> ok;
wait_all([P | Rest]) ->
    receive {done, P} -> wait_all(Rest) end.
