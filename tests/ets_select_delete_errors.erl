%% EXPECT: ets_select_delete_errors ok
-module(ets_select_delete_errors).
-export([test/0]).
test() ->
    ok = check(set),
    ok = check(ordered_set),
    ok = check(bag),
    ok = check(duplicate_bag),
    io:format("ets_select_delete_errors ok~n"),
    ok.
check(Type) ->
    T = ets:new(test, [Type]),
    true = ets:insert(T, [{a, 1}, {b, 2}, {c, 3}]),
    ok = bad(T, invalid_spec),
    ok = bad(T, [{'_'}]),
    ok = bad(T, [{'_', [], [true]} | invalid_tail]),
    ok = bad(T, [{'_', [], [true]}, invalid_clause]),
    ok = bad(T, [{{'_', '$1'}, [{unknown_guard, '$1', 2}], [true]}]),
    ok = bad(T, [{{'_', '$1'}, [{'<', '$2', 2}], [true]}]),
    %% Valid but unsupported constructs must also fail without deletion.
    ok = bad(T, [{'_', [], [true, true]}]),
    ok = bad(T, [{{a, '$1'}, [], [true]}]),
    ok = bad(T, [{{'$1', '$1'}, [], [true]}]),
    0 = ets:select_delete(T, [{{'_', '_', '_'}, [], [true]}]),
    0 = ets:select_delete(T, [{{'_', '$1'}, [{'=<', '$1', -1152921504606846976}], [true]}]),
    0 = ets:select_delete(T, [{{'_', '$1'}, [{'>=', '$1', 1152921504606846975}], [true]}]),
    0 = ets:select_delete(T, []),
    0 = ets:select_delete(T, [{'_', [], [false]}]),
    1 = ets:select_delete(T, [{{'_', '$1'}, [{'<', '$1', 2}], [true]}]),
    [{b, 2}] = ets:lookup(T, b),
    2 = ets:select_delete(T, [{'_', [], [true]}]),
    0 = ets:info(T, size),
    true = ets:delete(T),
    {'EXIT', {badarg, _}} = (catch ets:select_delete(T, [])),
    ok.
bad(T, Spec) ->
    {'EXIT', {badarg, _}} = (catch ets:select_delete(T, Spec)),
    3 = ets:info(T, size),
    ok.
