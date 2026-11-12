%% Iteration and aggregation patterns using first/next.
%% Exercises: first/1, next/2, lookup/2, tab2list/1,
%%            building aggregates without ets:foldl (not yet implemented).
-module(ets_iteration).
-export([test/0]).

test() ->
    test_sum_values(),
    test_filter_collect(),
    test_count_by_category(),
    test_transform_table(),
    test_empty_iteration(),
    ok.

%% Sum all numeric values in a table
test_sum_values() ->
    T = ets:new(sums, [set, public]),
    ets:insert(T, [{a, 10}, {b, 20}, {c, 30}, {d, 40}, {e, 50}]),

    150 = fold_values(T, ets:first(T), 0, fun(V, Acc) -> Acc + V end),

    ets:delete(T),
    ok.

%% Collect entries matching a condition
test_filter_collect() ->
    T = ets:new(scores, [set, public]),
    ets:insert(T, [
        {alice, 85},
        {bob, 42},
        {carol, 91},
        {dave, 73},
        {eve, 55},
        {frank, 98}
    ]),

    %% Collect students scoring above 80
    HighScorers = filter_collect(T, ets:first(T), [],
        fun({_Name, Score}) -> Score > 80 end),
    3 = length(HighScorers),
    true = lists:member({alice, 85}, HighScorers),
    true = lists:member({carol, 91}, HighScorers),
    true = lists:member({frank, 98}, HighScorers),

    %% Collect students scoring below 60
    LowScorers = filter_collect(T, ets:first(T), [],
        fun({_Name, Score}) -> Score < 60 end),
    2 = length(LowScorers),
    true = lists:member({bob, 42}, LowScorers),
    true = lists:member({eve, 55}, LowScorers),

    ets:delete(T),
    ok.

%% Count entries by category
test_count_by_category() ->
    T = ets:new(inventory, [set, public]),
    ets:insert(T, [
        {item1, fruit, 10},
        {item2, veggie, 5},
        {item3, fruit, 20},
        {item4, dairy, 8},
        {item5, veggie, 15},
        {item6, fruit, 3},
        {item7, dairy, 12}
    ]),

    %% Count fruits
    3 = count_matching(T, ets:first(T), 0,
        fun({_, Cat, _}) -> Cat =:= fruit end),
    %% Count veggies
    2 = count_matching(T, ets:first(T), 0,
        fun({_, Cat, _}) -> Cat =:= veggie end),
    %% Count dairy
    2 = count_matching(T, ets:first(T), 0,
        fun({_, Cat, _}) -> Cat =:= dairy end),

    %% Sum quantities of fruits
    33 = fold_entries(T, ets:first(T), 0,
        fun({_, fruit, Qty}, Acc) -> Acc + Qty;
           (_, Acc) -> Acc
        end),

    ets:delete(T),
    ok.

%% Read all entries, transform, write to a new table
test_transform_table() ->
    Src = ets:new(src, [set, public]),
    Dst = ets:new(dst, [set, public]),

    ets:insert(Src, [{x, 1}, {y, 2}, {z, 3}]),

    %% Copy with doubled values
    copy_transformed(Src, Dst, ets:first(Src),
        fun({K, V}) -> {K, V * 2} end),

    [{x, 2}] = ets:lookup(Dst, x),
    [{y, 4}] = ets:lookup(Dst, y),
    [{z, 6}] = ets:lookup(Dst, z),
    3 = ets:info(Dst, size),

    ets:delete(Src),
    ets:delete(Dst),
    ok.

%% Edge case: iterating empty table
test_empty_iteration() ->
    T = ets:new(empty, [set, public]),

    '$end_of_table' = ets:first(T),
    0 = fold_values(T, ets:first(T), 0, fun(V, Acc) -> Acc + V end),
    [] = filter_collect(T, ets:first(T), [], fun(_) -> true end),

    ets:delete(T),
    ok.

%% --- Generic iteration helpers ---

%% Fold over {Key, Value} entries, applying Fun(Value, Acc)
fold_values(_T, '$end_of_table', Acc, _Fun) -> Acc;
fold_values(T, Key, Acc, Fun) ->
    [{_, Val}] = ets:lookup(T, Key),
    fold_values(T, ets:next(T, Key), Fun(Val, Acc), Fun).

%% Fold over full entries, applying Fun(Entry, Acc)
fold_entries(_T, '$end_of_table', Acc, _Fun) -> Acc;
fold_entries(T, Key, Acc, Fun) ->
    [Entry] = ets:lookup(T, Key),
    fold_entries(T, ets:next(T, Key), Fun(Entry, Acc), Fun).

%% Collect entries matching Pred
filter_collect(_T, '$end_of_table', Acc, _Pred) -> Acc;
filter_collect(T, Key, Acc, Pred) ->
    [Entry] = ets:lookup(T, Key),
    NewAcc = case Pred(Entry) of
        true -> [Entry | Acc];
        false -> Acc
    end,
    filter_collect(T, ets:next(T, Key), NewAcc, Pred).

%% Count entries matching Pred
count_matching(_T, '$end_of_table', Acc, _Pred) -> Acc;
count_matching(T, Key, Acc, Pred) ->
    [Entry] = ets:lookup(T, Key),
    NewAcc = case Pred(Entry) of
        true -> Acc + 1;
        false -> Acc
    end,
    count_matching(T, ets:next(T, Key), NewAcc, Pred).

%% Copy entries from Src to Dst with transformation
copy_transformed(_Src, _Dst, '$end_of_table', _Fun) -> ok;
copy_transformed(Src, Dst, Key, Fun) ->
    [Entry] = ets:lookup(Src, Key),
    ets:insert(Dst, Fun(Entry)),
    copy_transformed(Src, Dst, ets:next(Src, Key), Fun).
