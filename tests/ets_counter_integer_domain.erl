%% EXPECT: ets_counter_integer_domain ok
-module(ets_counter_integer_domain).
-export([test/0]).
test() ->
    ok = check(set),
    ok = check(ordered_set),
    io:format("ets_counter_integer_domain ok~n"),
    ok.
check(Type) ->
    T = ets:new(test, [Type]),
    Key = {boxed, key},
    Max = 1152921504606846975,
    Min = -1152921504606846976,
    true = ets:insert(T, {Key, Max, Min, not_integer}),
    Old = ets:lookup(T, Key),
    A = Max + 1,
    A = ets:update_counter(T, Key, 1),
    B = Min - 1,
    B = ets:update_counter(T, Key, {3, -1}),
    Max = ets:update_counter(T, Key, -1),
    %% A later error must roll back a preceding overflowing operation.
    {'EXIT', {badarg, _}} = (catch ets:update_counter(T, Key, [{2, 1}, {4, 1}])),
    [{Key, Max, B, not_integer}] = ets:lookup(T, Key),
    [A, Max] = ets:update_counter(T, Key, [{2, 1}, {2, -1}]),
    %% Threshold wrapping is evaluated before choosing the representation.
    0 = ets:update_counter(T, Key, {2, 1, Max, 0}),
    Huge = 1 bsl 100,
    Huge = ets:update_counter(T, Key, {2, Huge}),
    [Huge] = ets:update_counter(T, Key, [{2, 0}]),
    0 = ets:update_counter(T, Key, {2, -Huge}),
    [0] = ets:update_counter(T, Key, [{2, 0}]),
    [] = ets:update_counter(T, Key, []),
    {'EXIT', {badarg, _}} = (catch ets:update_counter(T, Key, {1, 1})),
    {'EXIT', {badarg, _}} = (catch ets:update_counter(T, Key, [{2, 1} | bad_tail])),
    [{Key, 0, B, not_integer}] = ets:lookup(T, Key),
    true = ets:delete(T),
    [{Key, Max, Min, not_integer}] = Old,
    ok.
