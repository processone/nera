-module(ets_api).
-export([test/0]).

test() ->
    test_insert_new(),
    test_tab2list(),
    test_first_next(),
    test_info(),
    ok.

test_insert_new() ->
    T = ets:new(ins_test, [set, public]),
    %% First insert_new succeeds
    true = ets:insert_new(T, {key1, val1}),
    [{key1, val1}] = ets:lookup(T, key1),
    %% Second insert_new with same key fails
    false = ets:insert_new(T, {key1, val2}),
    %% Original value preserved
    [{key1, val1}] = ets:lookup(T, key1),
    %% Different key works
    true = ets:insert_new(T, {key2, val2}),
    [{key2, val2}] = ets:lookup(T, key2),
    ets:delete(T),
    ok.

test_tab2list() ->
    T = ets:new(tab_test, [set, public]),
    ets:insert(T, {a, 1}),
    ets:insert(T, {b, 2}),
    ets:insert(T, {c, 3}),
    List = ets:tab2list(T),
    3 = length(List),
    %% All entries should be present (order undefined for set)
    true = lists:member({a, 1}, List),
    true = lists:member({b, 2}, List),
    true = lists:member({c, 3}, List),
    %% Empty table → empty list
    T2 = ets:new(empty_tab, [set, public]),
    [] = ets:tab2list(T2),
    ets:delete(T),
    ets:delete(T2),
    ok.

test_first_next() ->
    T = ets:new(iter_test, [set, public]),
    %% Empty table
    '$end_of_table' = ets:first(T),
    %% Single entry
    ets:insert(T, {only, 42}),
    only = ets:first(T),
    '$end_of_table' = ets:next(T, only),
    %% Multiple entries — iterate all
    ets:insert(T, {alpha, 1}),
    ets:insert(T, {beta, 2}),
    ets:insert(T, {gamma, 3}),
    Keys = collect_keys(T, ets:first(T), []),
    4 = length(Keys),
    true = lists:member(only, Keys),
    true = lists:member(alpha, Keys),
    true = lists:member(beta, Keys),
    true = lists:member(gamma, Keys),
    ets:delete(T),
    ok.

collect_keys(_T, '$end_of_table', Acc) -> Acc;
collect_keys(T, Key, Acc) ->
    collect_keys(T, ets:next(T, Key), [Key | Acc]).

test_info() ->
    T = ets:new(info_test, [set, public]),
    ets:insert(T, {x, 1}),
    ets:insert(T, {y, 2}),
    %% info/2 specific items
    2 = ets:info(T, size),
    info_test = ets:info(T, name),
    set = ets:info(T, type),
    1 = ets:info(T, keypos),
    %% info/1 — proplist
    Info = ets:info(T),
    true = is_list(Info),
    {size, 2} = lists:keyfind(size, 1, Info),
    {name, info_test} = lists:keyfind(name, 1, Info),
    {type, set} = lists:keyfind(type, 1, Info),
    {keypos, 1} = lists:keyfind(keypos, 1, Info),
    ets:delete(T),
    ok.
