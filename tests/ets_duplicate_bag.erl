%% Tests for ETS duplicate_bag table type.
-module(ets_duplicate_bag).
-export([test/0]).

test() ->
    %% Create duplicate_bag table
    Tab = ets:new(test_dup_bag, [duplicate_bag, named_table]),

    %% Verify table type
    duplicate_bag = ets:info(Tab, type),

    %% Insert same tuple twice — both should be stored
    true = ets:insert(Tab, {key1, value1}),
    true = ets:insert(Tab, {key1, value1}),
    Result1 = ets:lookup(Tab, key1),
    2 = length(Result1),  %% both copies present

    %% Insert different values for same key
    true = ets:insert(Tab, {key1, value2}),
    Result2 = ets:lookup(Tab, key1),
    3 = length(Result2),

    %% Different keys work normally
    true = ets:insert(Tab, {key2, a}),
    true = ets:insert(Tab, {key2, a}),
    true = ets:insert(Tab, {key2, b}),
    Result3 = ets:lookup(Tab, key2),
    3 = length(Result3),

    %% member works
    true = ets:member(Tab, key1),
    true = ets:member(Tab, key2),
    false = ets:member(Tab, no_key),

    %% Size counts all tuples including duplicates
    6 = ets:info(Tab, size),

    %% Delete removes all entries for a key
    true = ets:delete(Tab, key1),
    [] = ets:lookup(Tab, key1),
    3 = ets:info(Tab, size),

    %% tab2list returns all tuples
    All = ets:tab2list(Tab),
    3 = length(All),

    io:format("All duplicate_bag tests passed!~n"),
    ok.
