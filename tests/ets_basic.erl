%% Tests for ETS module.
-module(ets_basic).
-export([test/0]).

test() ->
    %% Create a named ETS table
    Tab = ets:new(test_tab, [named_table, public, set]),

    %% Insert and lookup
    true = ets:insert(test_tab, {key1, value1}),
    [{key1, value1}] = ets:lookup(test_tab, key1),

    %% Lookup missing key
    [] = ets:lookup(test_tab, no_such_key),

    %% member
    true = ets:member(test_tab, key1),
    false = ets:member(test_tab, no_such_key),

    %% Overwrite existing key
    true = ets:insert(test_tab, {key1, new_value}),
    [{key1, new_value}] = ets:lookup(test_tab, key1),

    %% Insert multiple items
    true = ets:insert(test_tab, {key2, value2}),
    true = ets:insert(test_tab, {key3, value3}),
    [{key2, value2}] = ets:lookup(test_tab, key2),
    [{key3, value3}] = ets:lookup(test_tab, key3),

    %% Delete a key
    true = ets:delete(test_tab, key2),
    [] = ets:lookup(test_tab, key2),
    false = ets:member(test_tab, key2),

    %% Key still exists after deleting a different key
    [{key1, new_value}] = ets:lookup(test_tab, key1),

    %% Insert a tuple with more elements (custom keypos default=1)
    true = ets:insert(test_tab, {key4, val4a, val4b, val4c}),
    [{key4, val4a, val4b, val4c}] = ets:lookup(test_tab, key4),

    %% delete_all_objects
    true = ets:delete_all_objects(test_tab),
    [] = ets:lookup(test_tab, key1),
    [] = ets:lookup(test_tab, key3),
    [] = ets:lookup(test_tab, key4),

    %% Insert list of tuples
    true = ets:insert(test_tab, [{a, 1}, {b, 2}, {c, 3}]),
    [{a, 1}] = ets:lookup(test_tab, a),
    [{b, 2}] = ets:lookup(test_tab, b),
    [{c, 3}] = ets:lookup(test_tab, c),

    %% Clean up
    true = ets:delete_all_objects(test_tab),

    io:format("ok~n"),
    ok.
