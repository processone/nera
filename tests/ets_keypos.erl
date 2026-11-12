%% Tests for ETS with custom keypos.
-module(ets_keypos).
-export([test/0]).

test() ->
    %% Create table with keypos=2 (like onecached uses)
    Tab = ets:new(records, [named_table, public, set, {keypos, 2}]),

    %% Insert records where element 2 is the key
    true = ets:insert(records, {onecached, <<"session1">>, 0, 0, <<"data1">>}),
    true = ets:insert(records, {onecached, <<"session2">>, 0, 0, <<"data2">>}),

    %% Lookup by key (element 2)
    [{onecached, <<"session1">>, 0, 0, <<"data1">>}] = ets:lookup(records, <<"session1">>),
    [{onecached, <<"session2">>, 0, 0, <<"data2">>}] = ets:lookup(records, <<"session2">>),

    %% Missing key
    [] = ets:lookup(records, <<"nosuch">>),

    %% member
    true = ets:member(records, <<"session1">>),
    false = ets:member(records, <<"nosuch">>),

    %% Overwrite
    true = ets:insert(records, {onecached, <<"session1">>, 0, 0, <<"updated_data">>}),
    [{onecached, <<"session1">>, 0, 0, <<"updated_data">>}] = ets:lookup(records, <<"session1">>),

    %% Delete by key
    true = ets:delete(records, <<"session1">>),
    [] = ets:lookup(records, <<"session1">>),
    [{onecached, <<"session2">>, 0, 0, <<"data2">>}] = ets:lookup(records, <<"session2">>),

    io:format("ok~n"),
    ok.
