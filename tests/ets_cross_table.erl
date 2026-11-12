%% Tests that ETS lookup results can be re-inserted into another table.
%% Phase 5 regression baseline: with zero-copy, external pointers from
%% one table must be correctly deep-copied when inserted into another.
-module(ets_cross_table).
-export([test/0]).

test() ->
    T1 = ets:new(table1, [set, public]),
    T2 = ets:new(table2, [set, public]),

    %% Insert complex term into T1
    ets:insert(T1, {key1, [1, {nested, 42}]}),
    ets:insert(T1, {key2, list_to_binary(lists:duplicate(100, 67))}),

    %% Lookup from T1 and re-insert into T2 (with different keys)
    [{key1, Val1}] = ets:lookup(T1, key1),
    [{key2, Val2}] = ets:lookup(T1, key2),

    ets:insert(T2, {new_key1, Val1}),
    ets:insert(T2, {new_key2, Val2}),

    %% Lookup from T2 and verify data integrity
    [{new_key1, V1}] = ets:lookup(T2, new_key1),
    [1, {nested, 42}] = V1,

    [{new_key2, V2}] = ets:lookup(T2, new_key2),
    100 = byte_size(V2),

    %% Delete T1, verify T2 is independent
    ets:delete(T1),

    [{new_key1, V1b}] = ets:lookup(T2, new_key1),
    [1, {nested, 42}] = V1b,

    ets:delete(T2),
    ok.
