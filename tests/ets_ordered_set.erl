%% Tests for ETS ordered_set table type.
-module(ets_ordered_set).
-export([test/0]).

test() ->
    %% Create ordered_set table
    Tab = ets:new(test_ordered, [ordered_set, named_table]),

    %% Verify table type
    ordered_set = ets:info(Tab, type),

    %% Insert keys in random order
    true = ets:insert(Tab, {5, five}),
    true = ets:insert(Tab, {1, one}),
    true = ets:insert(Tab, {3, three}),
    true = ets:insert(Tab, {2, two}),
    true = ets:insert(Tab, {4, four}),

    %% Lookup works
    [{3, three}] = ets:lookup(Tab, 3),
    [] = ets:lookup(Tab, 99),

    %% member works
    true = ets:member(Tab, 1),
    false = ets:member(Tab, 99),

    %% first returns smallest key
    1 = ets:first(Tab),

    %% next returns keys in sorted order
    2 = ets:next(Tab, 1),
    3 = ets:next(Tab, 2),
    4 = ets:next(Tab, 3),
    5 = ets:next(Tab, 4),
    '$end_of_table' = ets:next(Tab, 5),

    %% last returns largest key
    5 = ets:last(Tab),

    %% prev returns keys in reverse sorted order
    4 = ets:prev(Tab, 5),
    3 = ets:prev(Tab, 4),
    2 = ets:prev(Tab, 3),
    1 = ets:prev(Tab, 2),
    '$end_of_table' = ets:prev(Tab, 1),

    %% Overwrite existing key
    true = ets:insert(Tab, {3, three_updated}),
    [{3, three_updated}] = ets:lookup(Tab, 3),
    5 = ets:info(Tab, size),

    %% Delete a key
    true = ets:delete(Tab, 3),
    [] = ets:lookup(Tab, 3),
    4 = ets:info(Tab, size),

    %% insert_new: fails if key exists
    false = ets:insert_new(Tab, {1, duplicate}),
    [{1, one}] = ets:lookup(Tab, 1),

    %% insert_new: succeeds if key is new
    true = ets:insert_new(Tab, {10, ten}),
    [{10, ten}] = ets:lookup(Tab, 10),

    %% Verify iteration still sorted after modifications
    1 = ets:first(Tab),
    2 = ets:next(Tab, 1),
    4 = ets:next(Tab, 2),
    5 = ets:next(Tab, 4),
    10 = ets:next(Tab, 5),
    '$end_of_table' = ets:next(Tab, 10),

    %% Mixed key types follow Erlang ordering: integers < atoms < tuples
    Tab2 = ets:new(test_mixed_order, [ordered_set]),
    true = ets:insert(Tab2, {{key, 1}, tuple_val}),
    true = ets:insert(Tab2, {42, int_val}),
    true = ets:insert(Tab2, {hello, atom_val}),
    42 = ets:first(Tab2),
    hello = ets:next(Tab2, 42),
    {key, 1} = ets:next(Tab2, hello),
    '$end_of_table' = ets:next(Tab2, {key, 1}),

    %% Empty table
    Tab3 = ets:new(test_empty_ordered, [ordered_set]),
    '$end_of_table' = ets:first(Tab3),
    '$end_of_table' = ets:last(Tab3),

    io:format("All ordered_set tests passed!~n"),
    ok.
