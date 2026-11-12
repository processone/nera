-module(ets_concurrent).
-export([test/0]).

test() ->
    test_concurrent_update_counter(),
    test_concurrent_insert_different_keys(),
    test_concurrent_insert_overwrite(),
    test_concurrent_read_write(),
    test_concurrent_delete_while_reading(),
    ok.

%% ---------------------------------------------------------------------------
%% Test 1: N processes increment the same counter — final value must be exact.
%% This catches lock bugs, lost updates, and atomicity violations.
%% ---------------------------------------------------------------------------
test_concurrent_update_counter() ->
    T = ets:new(counter_test, [set, public]),
    ets:insert(T, {counter, 0}),
    NumProcs = 10,
    IncrsPerProc = 1000,
    Parent = self(),
    Pids = [spawn(fun() ->
        counter_loop(T, IncrsPerProc),
        Parent ! {done, self()}
    end) || _ <- lists:seq(1, NumProcs)],
    wait_all(Pids),
    Expected = NumProcs * IncrsPerProc,
    [{counter, Expected}] = ets:lookup(T, counter),
    ets:delete(T),
    ok.

counter_loop(_, 0) -> ok;
counter_loop(T, N) ->
    ets:update_counter(T, counter, 1),
    counter_loop(T, N - 1).

%% ---------------------------------------------------------------------------
%% Test 2: N processes each insert their own key range — all entries present.
%% Catches concurrent HashMap insert corruption.
%% ---------------------------------------------------------------------------
test_concurrent_insert_different_keys() ->
    T = ets:new(insert_test, [set, public]),
    NumProcs = 5,
    KeysPerProc = 200,
    Parent = self(),
    Pids = [spawn(fun() ->
        Base = I * 10000,
        insert_range(T, Base, Base + KeysPerProc - 1),
        Parent ! {done, self()}
    end) || I <- lists:seq(1, NumProcs)],
    wait_all(Pids),
    %% Verify all entries present
    Expected = NumProcs * KeysPerProc,
    Expected = ets:info(T, size),
    %% Spot-check a few keys from each range
    lists:foreach(fun(I) ->
        Base = I * 10000,
        [{Base, Base}] = ets:lookup(T, Base),
        Last = Base + KeysPerProc - 1,
        [{Last, Last}] = ets:lookup(T, Last)
    end, lists:seq(1, NumProcs)),
    ets:delete(T),
    ok.

insert_range(_, From, To) when From > To -> ok;
insert_range(T, From, To) ->
    ets:insert(T, {From, From}),
    insert_range(T, From + 1, To).

%% ---------------------------------------------------------------------------
%% Test 3: Multiple processes overwrite the same key — last writer wins,
%% but no crash or corruption.
%% ---------------------------------------------------------------------------
test_concurrent_insert_overwrite() ->
    T = ets:new(overwrite_test, [set, public]),
    ets:insert(T, {shared_key, 0}),
    NumProcs = 5,
    WritesPerProc = 200,
    Parent = self(),
    Pids = [spawn(fun() ->
        overwrite_loop(T, I, WritesPerProc),
        Parent ! {done, self()}
    end) || I <- lists:seq(1, NumProcs)],
    wait_all(Pids),
    %% Key must exist with some value — no crash, no corruption
    [{shared_key, Val}] = ets:lookup(T, shared_key),
    true = is_integer(Val),
    ets:delete(T),
    ok.

overwrite_loop(_, _, 0) -> ok;
overwrite_loop(T, Id, N) ->
    ets:insert(T, {shared_key, Id * 1000 + N}),
    overwrite_loop(T, Id, N - 1).

%% ---------------------------------------------------------------------------
%% Test 4: One writer, multiple readers — readers must see consistent data.
%% ---------------------------------------------------------------------------
test_concurrent_read_write() ->
    T = ets:new(rw_test, [set, public]),
    %% Pre-populate
    lists:foreach(fun(I) ->
        ets:insert(T, {I, I})
    end, lists:seq(1, 100)),
    Parent = self(),
    %% Start readers
    ReaderPids = [spawn(fun() ->
        read_loop(T, 500),
        Parent ! {done, self()}
    end) || _ <- lists:seq(1, 3)],
    %% Start writer (overwrites + new keys)
    WriterPid = spawn(fun() ->
        write_loop(T, 1, 500),
        Parent ! {done, self()}
    end),
    wait_all([WriterPid | ReaderPids]),
    %% Table must still be consistent
    Size = ets:info(T, size),
    true = Size >= 100,  %% at least the original entries
    ets:delete(T),
    ok.

read_loop(_, 0) -> ok;
read_loop(T, N) ->
    Key = (N rem 100) + 1,
    _ = ets:lookup(T, Key),
    _ = ets:member(T, Key),
    read_loop(T, N - 1).

write_loop(_, _, 0) -> ok;
write_loop(T, I, N) ->
    ets:insert(T, {I, I * N}),
    write_loop(T, I + 1, N - 1).

%% ---------------------------------------------------------------------------
%% Test 5: Delete entries while another process reads — no crash.
%% ---------------------------------------------------------------------------
test_concurrent_delete_while_reading() ->
    T = ets:new(del_read_test, [set, public]),
    lists:foreach(fun(I) ->
        ets:insert(T, {I, I * 10})
    end, lists:seq(1, 500)),
    Parent = self(),
    %% Reader: iterate via first/next
    ReaderPid = spawn(fun() ->
        iterate_all(T, ets:first(T), 0),
        Parent ! {done, self()}
    end),
    %% Deleter: delete half the keys
    DeleterPid = spawn(fun() ->
        delete_evens(T, 2, 500),
        Parent ! {done, self()}
    end),
    wait_all([ReaderPid, DeleterPid]),
    %% Odd keys must still exist
    lists:foreach(fun(I) ->
        [{I, _}] = ets:lookup(T, I)
    end, lists:seq(1, 500, 2)),
    ets:delete(T),
    ok.

iterate_all(_, '$end_of_table', _Count) -> ok;
iterate_all(T, Key, Count) ->
    _ = ets:lookup(T, Key),
    Next = ets:next(T, Key),
    iterate_all(T, Next, Count + 1).

delete_evens(_, From, To) when From > To -> ok;
delete_evens(T, From, To) ->
    ets:delete(T, From),
    delete_evens(T, From + 2, To).

%% ---------------------------------------------------------------------------
%% Helpers
%% ---------------------------------------------------------------------------
wait_all([]) -> ok;
wait_all([Pid | Rest]) ->
    receive
        {done, Pid} -> wait_all(Rest)
    end.
