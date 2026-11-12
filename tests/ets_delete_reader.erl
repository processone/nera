%% Tests that a process holding an ETS lookup result is unaffected
%% when the key is deleted from the table.
%% Phase 5 regression baseline: with zero-copy (Arc sharing), the Arc
%% must keep the CompactHeap alive even after the ETS entry is removed.
-module(ets_delete_reader).
-export([test/0]).

test() ->
    T = ets:new(delete_reader_test, [set, public]),

    %% Insert entries with complex values
    ets:insert(T, {key1, [10, 20, 30]}),
    ets:insert(T, {key2, {nested, {inner, 42}}}),
    ets:insert(T, {key3, list_to_binary(lists:duplicate(100, 68))}),

    Parent = self(),

    %% Spawn reader that looks up and holds all three values
    spawn(fun() ->
        [{key1, V1}] = ets:lookup(T, key1),
        [{key2, V2}] = ets:lookup(T, key2),
        [{key3, V3}] = ets:lookup(T, key3),
        Parent ! {reader_ready, self()},

        %% Wait for deletions
        receive go_verify -> ok end,

        %% Verify all held results are still valid
        [10, 20, 30] = V1,
        {nested, {inner, 42}} = V2,
        100 = byte_size(V3),

        %% Verify keys are gone from ETS
        [] = ets:lookup(T, key1),
        [] = ets:lookup(T, key2),
        [] = ets:lookup(T, key3),

        Parent ! {reader_done, self()}
    end),

    ReaderPid = receive {reader_ready, Pid} -> Pid end,

    %% Delete all keys
    ets:delete(T, key1),
    ets:delete(T, key2),
    ets:delete(T, key3),

    %% Tell reader to verify
    ReaderPid ! go_verify,

    receive {reader_done, ReaderPid} -> ok end,

    ets:delete(T),
    ok.
