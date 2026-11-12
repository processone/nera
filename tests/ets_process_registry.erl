%% Cross-process ETS access inspired by Armstrong's shared state examples.
%% Exercises: named_table across processes, concurrent reads/writes,
%%            update_counter from multiple processes, info/2 size tracking.
-module(ets_process_registry).
-export([test/0]).

test() ->
    test_shared_read(),
    test_multi_writer(),
    test_concurrent_counter(),
    ok.

%% One process populates, spawned processes read
test_shared_read() ->
    T = ets:new(shared_db, [named_table, set, public]),

    ets:insert(shared_db, {user1, "Alice", admin}),
    ets:insert(shared_db, {user2, "Bob", member}),
    ets:insert(shared_db, {user3, "Carol", admin}),

    Self = self(),

    %% Spawn readers that verify data
    spawn(fun() ->
        [{user1, "Alice", admin}] = ets:lookup(shared_db, user1),
        Self ! {reader1, ok}
    end),
    spawn(fun() ->
        [{user2, "Bob", member}] = ets:lookup(shared_db, user2),
        Self ! {reader2, ok}
    end),
    spawn(fun() ->
        [{user3, "Carol", admin}] = ets:lookup(shared_db, user3),
        Self ! {reader3, ok}
    end),

    %% Wait for all readers
    receive {reader1, ok} -> ok end,
    receive {reader2, ok} -> ok end,
    receive {reader3, ok} -> ok end,

    ets:delete(shared_db),
    ok.

%% Multiple processes write to the same table
test_multi_writer() ->
    T = ets:new(multi_w, [set, public]),
    Self = self(),
    N = 20,

    %% Each process inserts a unique key
    Pids = spawn_writers(T, Self, N, 1, []),

    %% Wait for all writers
    wait_all(Pids),

    %% All entries should be present
    N = ets:info(T, size),
    verify_entries(T, N, 1),

    ets:delete(T),
    ok.

%% Multiple processes increment the same counter
test_concurrent_counter() ->
    T = ets:new(cc_tab, [set, public]),
    ets:insert(T, {hits, 0}),
    Self = self(),

    NumProcs = 5,
    IncrsPerProc = 100,

    %% Spawn processes that each increment N times
    Pids = spawn_counters(T, Self, NumProcs, IncrsPerProc, []),
    wait_all(Pids),

    %% Final value should be NumProcs * IncrsPerProc
    Expected = NumProcs * IncrsPerProc,
    [{hits, Expected}] = ets:lookup(T, hits),

    ets:delete(T),
    ok.

%% --- Helpers ---

spawn_writers(_T, _Self, 0, _I, Acc) -> Acc;
spawn_writers(T, Self, N, I, Acc) ->
    Pid = spawn(fun() ->
        ets:insert(T, {I, I * 10}),
        Self ! {done, self()}
    end),
    spawn_writers(T, Self, N - 1, I + 1, [Pid | Acc]).

spawn_counters(_T, _Self, 0, _N, Acc) -> Acc;
spawn_counters(T, Self, Procs, N, Acc) ->
    Pid = spawn(fun() ->
        incr_loop(T, N),
        Self ! {done, self()}
    end),
    spawn_counters(T, Self, Procs - 1, N, [Pid | Acc]).

incr_loop(_T, 0) -> ok;
incr_loop(T, N) ->
    ets:update_counter(T, hits, 1),
    incr_loop(T, N - 1).

wait_all([]) -> ok;
wait_all([Pid | Rest]) ->
    receive {done, Pid} -> ok end,
    wait_all(Rest).

verify_entries(_T, 0, _I) -> ok;
verify_entries(T, N, I) ->
    [{I, Val}] = ets:lookup(T, I),
    Val = I * 10,
    verify_entries(T, N - 1, I + 1).
