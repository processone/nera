%% Tests for ETS bag table type.
-module(ets_bag).
-export([test/0]).

test() ->
    %% Create bag table
    Tab = ets:new(test_bag, [bag, named_table]),

    %% Verify table type
    bag = ets:info(Tab, type),

    %% Insert multiple tuples with same key
    true = ets:insert(Tab, {key1, value1}),
    true = ets:insert(Tab, {key1, value2}),
    true = ets:insert(Tab, {key1, value3}),

    %% Lookup returns all matching tuples
    Result1 = ets:lookup(Tab, key1),
    3 = length(Result1),

    %% Verify all values are present
    true = lists:member({key1, value1}, Result1),
    true = lists:member({key1, value2}, Result1),
    true = lists:member({key1, value3}, Result1),

    %% Exact duplicate tuples are suppressed (inserted once)
    true = ets:insert(Tab, {key1, value1}),
    Result2 = ets:lookup(Tab, key1),
    3 = length(Result2),  %% still 3, not 4

    %% Different key works normally
    true = ets:insert(Tab, {key2, val_a}),
    [{key2, val_a}] = ets:lookup(Tab, key2),

    %% member works
    true = ets:member(Tab, key1),
    true = ets:member(Tab, key2),
    false = ets:member(Tab, no_key),

    %% Delete by key removes all tuples with that key
    true = ets:delete(Tab, key1),
    [] = ets:lookup(Tab, key1),
    false = ets:member(Tab, key1),

    %% key2 still there
    [{key2, val_a}] = ets:lookup(Tab, key2),

    %% Size counts individual tuples
    true = ets:insert(Tab, {key3, a}),
    true = ets:insert(Tab, {key3, b}),
    true = ets:insert(Tab, {key3, c}),
    %% key2 has 1, key3 has 3 = 4 total
    4 = ets:info(Tab, size),

    %% tab2list returns all tuples
    All = ets:tab2list(Tab),
    4 = length(All),

    io:format("All bag tests passed!~n"),
    ok.
