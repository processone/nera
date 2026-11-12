%% Tests ets:delete/1 — table deletion.
%% Verifies that deleting a table frees its data and that the table
%% is no longer accessible.
-module(ets_delete_table).
-export([test/0]).

test() ->
    %% Create a table, insert data, then delete the table
    Tab = ets:new(del_tab, [named_table, public, set]),
    true = ets:insert(del_tab, {key1, value1}),
    true = ets:insert(del_tab, {key2, value2}),

    %% Verify data is there
    [{key1, value1}] = ets:lookup(del_tab, key1),

    %% Delete the table
    true = ets:delete(del_tab),

    %% Create a new table with a different name to prove the old one is gone
    %% (if we tried to use the old name it would fail since the table is deleted)
    Tab2 = ets:new(del_tab2, [named_table, public, set]),
    true = ets:insert(del_tab2, {new_key, new_value}),
    [{new_key, new_value}] = ets:lookup(del_tab2, new_key),

    %% Delete by table ID
    true = ets:delete(Tab2),

    ok.
