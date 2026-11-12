%% Tests ported from OTP ets_SUITE.erl: edge cases, empty tables, delete semantics.
%% Adapted to nera test convention (test/0 -> ok, badmatch = failure).
-module(ets_otp_edge_cases).
-export([test/0]).

test() ->
    test_empty_table(),
    test_delete_nonexistent(),
    test_delete_all_refill(),
    test_member_boundaries(),
    test_mixed_key_types(),
    test_large_tuple(),
    test_info_defaults(),
    test_select_delete_guards(),
    ok.

%% OTP: comprehensive empty table behavior
test_empty_table() ->
    T = ets:new(t, [set, public]),

    %% All operations on empty table
    0 = ets:info(T, size),
    [] = ets:tab2list(T),
    '$end_of_table' = ets:first(T),
    [] = ets:lookup(T, any_key),
    false = ets:member(T, any_key),

    %% delete_all_objects on empty table
    true = ets:delete_all_objects(T),
    0 = ets:info(T, size),

    %% insert_new on empty table
    true = ets:insert_new(T, {first, 1}),
    1 = ets:info(T, size),

    ets:delete(T),
    ok.

%% OTP: delete of non-existent key returns true
test_delete_nonexistent() ->
    T = ets:new(t, [set, public]),

    ets:insert(T, {a, 1}),
    ets:insert(T, {b, 2}),

    %% Delete existing
    true = ets:delete(T, a),
    1 = ets:info(T, size),

    %% Delete non-existent — still returns true
    true = ets:delete(T, a),
    true = ets:delete(T, nonexistent),
    true = ets:delete(T, 999),
    1 = ets:info(T, size),

    %% Remaining entry unaffected
    [{b, 2}] = ets:lookup(T, b),

    ets:delete(T),
    ok.

%% OTP t_delete_all_objects: clear, refill, clear cycle
test_delete_all_refill() ->
    T = ets:new(t, [set, public]),
    N = 100,

    %% Fill
    fill_tab(T, 1, N),
    N = ets:info(T, size),

    %% Clear
    true = ets:delete_all_objects(T),
    0 = ets:info(T, size),
    [] = ets:lookup(T, 1),
    '$end_of_table' = ets:first(T),

    %% Refill
    fill_tab(T, 1, N),
    N = ets:info(T, size),

    %% Verify some entries
    [{1, 10}] = ets:lookup(T, 1),
    [{50, 500}] = ets:lookup(T, 50),
    [{100, 1000}] = ets:lookup(T, 100),

    %% Clear again
    true = ets:delete_all_objects(T),
    0 = ets:info(T, size),

    ets:delete(T),
    ok.

%% OTP member_do: boundary checks on integer keys
test_member_boundaries() ->
    T = ets:new(t, [set, public]),
    N = 100,
    fill_tab(T, 1, N),

    %% Within range
    true = ets:member(T, 1),
    true = ets:member(T, 50),
    true = ets:member(T, 100),

    %% Out of range
    false = ets:member(T, 0),
    false = ets:member(T, 101),
    false = ets:member(T, -1),
    false = ets:member(T, 999),

    %% After delete
    ets:delete(T, 50),
    false = ets:member(T, 50),
    true = ets:member(T, 49),
    true = ets:member(T, 51),

    ets:delete(T),
    ok.

%% OTP: different key types in the same set table
test_mixed_key_types() ->
    T = ets:new(t, [set, public]),

    %% Atom keys
    ets:insert(T, {hello, "atom_key"}),
    [{hello, "atom_key"}] = ets:lookup(T, hello),

    %% Integer keys
    ets:insert(T, {42, "int_key"}),
    [{42, "int_key"}] = ets:lookup(T, 42),

    %% Negative integer key
    ets:insert(T, {-7, "neg_key"}),
    [{-7, "neg_key"}] = ets:lookup(T, -7),

    %% Zero key
    ets:insert(T, {0, "zero_key"}),
    [{0, "zero_key"}] = ets:lookup(T, 0),

    %% Tuple key
    ets:insert(T, {{a, b}, "tuple_key"}),
    [{{a, b}, "tuple_key"}] = ets:lookup(T, {a, b}),

    %% All coexist
    5 = ets:info(T, size),
    true = ets:member(T, hello),
    true = ets:member(T, 42),
    true = ets:member(T, -7),
    true = ets:member(T, 0),
    true = ets:member(T, {a, b}),

    %% Delete by each type
    ets:delete(T, {a, b}),
    false = ets:member(T, {a, b}),
    4 = ets:info(T, size),

    ets:delete(T, -7),
    false = ets:member(T, -7),
    3 = ets:info(T, size),

    ets:delete(T),
    ok.

%% OTP: large tuples with many elements
test_large_tuple() ->
    T = ets:new(t, [set, public]),

    %% 10-element tuple
    Big = {key, 1, 2, 3, 4, 5, 6, 7, 8, 9},
    ets:insert(T, Big),
    [Big] = ets:lookup(T, key),

    %% Update various positions
    ets:update_element(T, key, {5, 50}),
    ets:update_element(T, key, {10, 90}),
    [{key, 1, 2, 3, 50, 5, 6, 7, 8, 90}] = ets:lookup(T, key),

    %% Nested tuple values
    ets:insert(T, {nested, {a, {b, {c, d}}}, [1, 2, 3]}),
    [{nested, {a, {b, {c, d}}}, [1, 2, 3]}] = ets:lookup(T, nested),

    ets:delete(T),
    ok.

%% OTP default: info/2 properties of new table
test_info_defaults() ->
    T = ets:new(my_table, [set, public]),

    set = ets:info(T, type),
    1 = ets:info(T, keypos),
    my_table = ets:info(T, name),
    0 = ets:info(T, size),

    %% info/1 returns proplist
    Info = ets:info(T),
    true = is_list(Info),
    {type, set} = lists:keyfind(type, 1, Info),
    {keypos, 1} = lists:keyfind(keypos, 1, Info),
    {name, my_table} = lists:keyfind(name, 1, Info),
    {size, 0} = lists:keyfind(size, 1, Info),

    %% Size updates
    ets:insert(T, {x, 1}),
    1 = ets:info(T, size),
    {size, 1} = lists:keyfind(size, 1, ets:info(T)),

    %% Custom keypos
    T2 = ets:new(kp3, [set, public, {keypos, 3}]),
    3 = ets:info(T2, keypos),
    {keypos, 3} = lists:keyfind(keypos, 1, ets:info(T2)),

    ets:delete(T),
    ets:delete(T2),
    ok.

%% OTP t_select_delete: test both '<' and '>' guards
test_select_delete_guards() ->
    T = ets:new(t, [set, public]),

    %% {Key, Score}
    ets:insert(T, [{a, 10}, {b, 30}, {c, 50}, {d, 70}, {e, 90}]),

    %% Delete entries where score > 60 ($1 bound at position 2)
    2 = ets:select_delete(T, [{{'_', '$1'}, [{'>', '$1', 60}], [true]}]),
    3 = ets:info(T, size),
    true = ets:member(T, a),
    true = ets:member(T, b),
    true = ets:member(T, c),
    false = ets:member(T, d),
    false = ets:member(T, e),

    %% Delete entries where score =< 20 (implemented as < 21)
    1 = ets:select_delete(T, [{{'_', '$1'}, [{'=<', '$1', 20}], [true]}]),
    2 = ets:info(T, size),
    false = ets:member(T, a),
    true = ets:member(T, b),
    true = ets:member(T, c),

    %% Delete entries where score >= 30 (implemented as > 29)
    2 = ets:select_delete(T, [{{'_', '$1'}, [{'>=', '$1', 30}], [true]}]),
    0 = ets:info(T, size),

    ets:delete(T),
    ok.

%% --- Helpers ---

fill_tab(_T, I, Max) when I > Max -> ok;
fill_tab(T, I, Max) ->
    ets:insert(T, {I, I * 10}),
    fill_tab(T, I + 1, Max).
