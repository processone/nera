-module(bench_counters).
-export([start/0]).

%% counters microbenchmark — measures add, get, concurrent add,
%% multi-index ops, and comparison with ets:update_counter.
%% Compatible with both nera and BEAM.

-define(N, 100000).

start() ->
    Ref = counters:new(16, [atomics]),

    %% 1. Sequential add on single counter
    T0_add = erlang:monotonic_time(millisecond),
    add_n(Ref, 1, ?N),
    T1_add = erlang:monotonic_time(millisecond),
    Val = counters:get(Ref, 1),

    %% 2. Sequential get
    T0_get = erlang:monotonic_time(millisecond),
    get_n(Ref, 1, ?N),
    T1_get = erlang:monotonic_time(millisecond),

    %% 3. Concurrent add: 4 processes × N/4 adds to same counter
    counters:put(Ref, 2, 0),
    T0_conc = erlang:monotonic_time(millisecond),
    concurrent_add(Ref, 2, 4, ?N div 4),
    T1_conc = erlang:monotonic_time(millisecond),
    ConcVal = counters:get(Ref, 2),

    %% 4. Multi-index: N ops spread across 16 indices
    T0_multi = erlang:monotonic_time(millisecond),
    multi_add(Ref, ?N, 16),
    T1_multi = erlang:monotonic_time(millisecond),

    %% 5. Comparison: ets:update_counter on same workload
    T = ets:new(bench_ctr, [set, public]),
    ets:insert(T, {counter, 0}),
    T0_ets = erlang:monotonic_time(millisecond),
    ets_counter_n(T, counter, ?N),
    T1_ets = erlang:monotonic_time(millisecond),
    [{counter, EtsVal}] = ets:lookup(T, counter),
    ets:delete(T),

    %% Report
    io:format("counters Benchmark (~p ops)~n", [?N]),
    io:format("  seq add:        ~p ms  (result=~p)~n", [T1_add - T0_add, Val]),
    io:format("  seq get:        ~p ms~n", [T1_get - T0_get]),
    io:format("  conc_add:       ~p ms  (4 procs, result=~p)~n", [T1_conc - T0_conc, ConcVal]),
    io:format("  multi_add(16):  ~p ms~n", [T1_multi - T0_multi]),
    io:format("  ets:update_ctr: ~p ms  (result=~p)~n", [T1_ets - T0_ets, EtsVal]),
    halt(0).

add_n(_, _, 0) -> ok;
add_n(Ref, Ix, N) ->
    counters:add(Ref, Ix, 1),
    add_n(Ref, Ix, N - 1).

get_n(_, _, 0) -> ok;
get_n(Ref, Ix, N) ->
    _ = counters:get(Ref, Ix),
    get_n(Ref, Ix, N - 1).

multi_add(_, 0, _) -> ok;
multi_add(Ref, N, Size) ->
    Ix = (N rem Size) + 1,
    counters:add(Ref, Ix, 1),
    multi_add(Ref, N - 1, Size).

concurrent_add(Ref, Ix, NumProcs, AddsPerProc) ->
    Parent = self(),
    Pids = [spawn(fun() ->
        add_n(Ref, Ix, AddsPerProc),
        Parent ! {done, self()}
    end) || _ <- lists:seq(1, NumProcs)],
    wait_all(Pids).

ets_counter_n(_, _, 0) -> ok;
ets_counter_n(T, Key, N) ->
    ets:update_counter(T, Key, 1),
    ets_counter_n(T, Key, N - 1).

wait_all([]) -> ok;
wait_all([P | Rest]) ->
    receive {done, P} -> wait_all(Rest) end.
