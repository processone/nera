-module(bench_persistent_term).
-export([start/0]).

%% persistent_term microbenchmark — measures put, get, overwrite, erase,
%% concurrent reads, and mixed read/write workloads.
%% Compatible with both nera and BEAM.

-define(N, 50000).

start() ->
    %% 1. Sequential put (unique keys)
    T0_put = erlang:monotonic_time(millisecond),
    put_n(?N),
    T1_put = erlang:monotonic_time(millisecond),

    %% 2. Sequential get (all keys)
    T0_get = erlang:monotonic_time(millisecond),
    get_n(?N),
    T1_get = erlang:monotonic_time(millisecond),

    %% 3. Overwrite (re-put all keys with new values)
    T0_over = erlang:monotonic_time(millisecond),
    overwrite_n(?N),
    T1_over = erlang:monotonic_time(millisecond),

    %% 4. Get with default (mix of hit/miss)
    T0_get2 = erlang:monotonic_time(millisecond),
    get_default_n(?N),
    T1_get2 = erlang:monotonic_time(millisecond),

    %% 5. Concurrent read: 4 processes × N/4 gets each
    T0_conc = erlang:monotonic_time(millisecond),
    concurrent_get(4, ?N div 4),
    T1_conc = erlang:monotonic_time(millisecond),

    %% 6. Sequential erase
    T0_erase = erlang:monotonic_time(millisecond),
    erase_n(?N),
    T1_erase = erlang:monotonic_time(millisecond),

    %% Report
    io:format("persistent_term Benchmark (~p entries)~n", [?N]),
    io:format("  put:            ~p ms~n", [T1_put - T0_put]),
    io:format("  get:            ~p ms~n", [T1_get - T0_get]),
    io:format("  overwrite:      ~p ms~n", [T1_over - T0_over]),
    io:format("  get/2 (default):~p ms~n", [T1_get2 - T0_get2]),
    io:format("  conc_get:       ~p ms  (4 procs)~n", [T1_conc - T0_conc]),
    io:format("  erase:          ~p ms~n", [T1_erase - T0_erase]),
    halt(0).

put_n(0) -> ok;
put_n(N) ->
    persistent_term:put({bench_key, N}, {value, N, N * 2}),
    put_n(N - 1).

get_n(0) -> ok;
get_n(N) ->
    _ = persistent_term:get({bench_key, N}),
    get_n(N - 1).

overwrite_n(0) -> ok;
overwrite_n(N) ->
    persistent_term:put({bench_key, N}, {new_value, N}),
    overwrite_n(N - 1).

get_default_n(0) -> ok;
get_default_n(N) ->
    %% Even keys hit, odd keys miss (since we only stored even offsets? no, all exist)
    _ = persistent_term:get({bench_key, N}, default),
    %% Also try a miss
    _ = persistent_term:get({bench_miss, N}, default),
    get_default_n(N - 1).

erase_n(0) -> ok;
erase_n(N) ->
    persistent_term:erase({bench_key, N}),
    erase_n(N - 1).

concurrent_get(NumProcs, GetsPerProc) ->
    Parent = self(),
    Pids = [spawn(fun() ->
        get_n(GetsPerProc),
        Parent ! {done, self()}
    end) || _ <- lists:seq(1, NumProcs)],
    wait_all(Pids).

wait_all([]) -> ok;
wait_all([P | Rest]) ->
    receive {done, P} -> wait_all(Rest) end.
