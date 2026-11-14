%% EXPECT: ets_bitstring_keys_ok
%% Regression for storage copies exposing pointer-based bitstring keys.
%% Returned/copied keys and bag values must still compare by significant bits,
%% including bitstrings nested in lists and tuples. Run unchanged on BEAM.
-module(ets_bitstring_keys).
-export([test/0]).

test() ->
    bag_duplicates(),
    [table_keys(Type) || Type <- [set, bag, duplicate_bag]],
    persistent_keys(),
    io:format("ets_bitstring_keys_ok~n"),
    ok.

keys() ->
    Small = <<"abcdefgh", (id(5)):4>>,
    Bytes = list_to_binary(lists:duplicate(id(200), $x)),
    Large = <<Bytes/binary, (id(3)):2>>,
    [Small, Large, [Small], {wrapped, [Large, Small]},
     <<(id(5)):3>>, <<(id(20)):5>>, <<(id(4)):3>>].

id(X) -> X.

bag_duplicates() ->
    [Small, Large | _] = keys(),
    Value = {Small, [Large, Small]},
    T = ets:new(bitstring_bag, [bag]),
    Row = {key, Value},
    true = ets:insert(T, Row),
    true = ets:insert(T, Row),
    [Row] = ets:lookup(T, key),
    true = ets:delete(T),
    ok.

table_keys(Type) ->
    T = ets:new(bitstring_keys, [Type]),
    Keys = keys(),
    [true = ets:insert(T, {K, 0, unchanged}) || K <- Keys],
    CopiedRows = ets:tab2list(T),
    Count = length(Keys),
    Count = length(CopiedRows),
    [check_copied_key(Type, T, Row) || Row <- CopiedRows],
    Sorted = lists:sort(Keys),
    Sorted = lists:sort(walk(T, ets:first(T), next)),
    Sorted = lists:sort(walk(T, ets:last(T), prev)),
    %% Keys from traversal also work for lookup and membership.
    true = ets:member(T, ets:first(T)),
    true = ets:member(T, ets:last(T)),
    [begin
         true = ets:delete(T, K),
         false = ets:member(T, K),
         [] = ets:lookup(T, K)
     end || {K, _, _} <- CopiedRows],
    [] = ets:tab2list(T),
    true = ets:delete(T),
    ok.

check_copied_key(Type, T, {K, 0, unchanged}) ->
    true = ets:member(T, K),
    [{K, 0, unchanged}] = ets:lookup(T, K),
    false = ets:insert_new(T, {K, 99, replaced}),
    case Type of
        set ->
            true = ets:update_element(T, K, {3, [updated]}),
            1 = ets:update_counter(T, K, {2, 1}),
            [{K, 1, [updated]}] = ets:lookup(T, K);
        _ -> ok
    end,
    ok.

walk(_T, '$end_of_table', _Direction) -> [];
walk(T, K, Direction) ->
    [_] = ets:lookup(T, K),
    Next = case Direction of
        next -> ets:next(T, K);
        prev -> ets:prev(T, K)
    end,
    [K | walk(T, Next, Direction)].

persistent_keys() ->
    CopyKey = {?MODULE, copied_key},
    [begin
         %% Reading a stored value makes a new descriptor, which must be
         %% interchangeable with the original when either is used as a key.
         ok = persistent_term:put(CopyKey, K),
         Copied = persistent_term:get(CopyKey),
         ok = persistent_term:put(K, original),
         original = persistent_term:get(Copied),
         ok = persistent_term:put(Copied, replaced),
         replaced = persistent_term:get(K),
         true = persistent_term:erase(Copied),
         missing = persistent_term:get(K, missing)
     end || K <- keys()],
    true = persistent_term:erase(CopyKey),
    ok.
