-module(bench_ets).
-export([start/0]).

%% ETS microbenchmark — measures insert, lookup, update_counter, iterate, delete.
%% Uses erlang:monotonic_time(millisecond) for internal timing (separates from startup).
%% Compatible with both nera and BEAM.

-define(N, 50000).

start() ->
    T = ets:new(bench, [set, public]),

    %% 1. Sequential insert
    T0_insert = erlang:monotonic_time(millisecond),
    insert_n(T, ?N),
    T1_insert = erlang:monotonic_time(millisecond),

    %% 2. Sequential lookup (all keys)
    T0_lookup = erlang:monotonic_time(millisecond),
    lookup_n(T, ?N),
    T1_lookup = erlang:monotonic_time(millisecond),

    %% 3. update_counter on single key
    ets:insert(T, {counter_key, 0}),
    T0_counter = erlang:monotonic_time(millisecond),
    counter_n(T, ?N),
    T1_counter = erlang:monotonic_time(millisecond),

    %% 4. Overwrite (re-insert all keys with new values)
    T0_overwrite = erlang:monotonic_time(millisecond),
    overwrite_n(T, ?N),
    T1_overwrite = erlang:monotonic_time(millisecond),

    %% 5. Iterate all via tab2list (O(N), preferred for full scans)
    T0_iter = erlang:monotonic_time(millisecond),
    AllEntries = ets:tab2list(T),
    IterCount = length(AllEntries),
    T1_iter = erlang:monotonic_time(millisecond),

    %% 6. Delete all keys one by one
    T0_delete = erlang:monotonic_time(millisecond),
    delete_n(T, ?N),
    T1_delete = erlang:monotonic_time(millisecond),

    %% 7. Concurrent update_counter: 4 processes × N/4 increments each
    ets:insert(T, {conc_counter, 0}),
    T0_conc = erlang:monotonic_time(millisecond),
    concurrent_counter(T, 4, ?N div 4),
    T1_conc = erlang:monotonic_time(millisecond),
    [{conc_counter, ConcVal}] = ets:lookup(T, conc_counter),

    ets:delete(T),

    %% Report
    io:format("ETS Benchmark (~p entries)~n", [?N]),
    io:format("  insert:         ~p ms~n", [T1_insert - T0_insert]),
    io:format("  lookup:         ~p ms~n", [T1_lookup - T0_lookup]),
    io:format("  update_counter: ~p ms  (~p ops)~n", [T1_counter - T0_counter, ?N]),
    io:format("  overwrite:      ~p ms~n", [T1_overwrite - T0_overwrite]),
    io:format("  iterate:        ~p ms  (~p keys)~n", [T1_iter - T0_iter, IterCount]),
    io:format("  delete:         ~p ms~n", [T1_delete - T0_delete]),
    io:format("  conc_counter:   ~p ms  (4 procs, result=~p)~n", [T1_conc - T0_conc, ConcVal]),
    halt(0).

insert_n(_, 0) -> ok;
insert_n(T, N) ->
    ets:insert(T, {N, N, N * 2}),
    insert_n(T, N - 1).

lookup_n(_, 0) -> ok;
lookup_n(T, N) ->
    [{N, N, _}] = ets:lookup(T, N),
    lookup_n(T, N - 1).

counter_n(T, N) -> counter_n(T, counter_key, N).

counter_n(_, _, 0) -> ok;
counter_n(T, Key, N) ->
    ets:update_counter(T, Key, 1),
    counter_n(T, Key, N - 1).

overwrite_n(_, 0) -> ok;
overwrite_n(T, N) ->
    ets:insert(T, {N, N + 1, N * 3}),
    overwrite_n(T, N - 1).

iterate_all(_, '$end_of_table', Count) -> Count;
iterate_all(T, Key, Count) ->
    iterate_all(T, ets:next(T, Key), Count + 1).

delete_n(_, 0) -> ok;
delete_n(T, N) ->
    ets:delete(T, N),
    delete_n(T, N - 1).

concurrent_counter(T, NumProcs, IncrsPerProc) ->
    Parent = self(),
    Pids = [spawn(fun() ->
        counter_n(T, conc_counter, IncrsPerProc),
        Parent ! {done, self()}
    end) || _ <- lists:seq(1, NumProcs)],
    wait_all(Pids).

wait_all([]) -> ok;
wait_all([P | Rest]) ->
    receive {done, P} -> wait_all(Rest) end.
