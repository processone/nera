-module(ets_counter_atomicity).
-export([test/0]).

test() ->
    check(set),
    check(ordered_set),
    ok.

check(Type) ->
    T = ets:new(counter_atomicity, [Type]),
    true = ets:insert(T, {key, 10, not_a_counter}),
    unchanged(T, [{2, 5}, {3, 1}]),
    unchanged(T, [{2, 5}, {4, 1}]),
    unchanged(T, [{2, 5}, {0, 1}]),
    %% Repeated positions observe previous operations, including threshold wrap.
    [15, 0, -2] = ets:update_counter(T, key, [{2, 5}, {2, 1, 15, 0}, {2, -2}]),
    [{key, -2, not_a_counter}] = ets:lookup(T, key),
    true = ets:delete(T),
    ok.

unchanged(T, Ops) ->
    {'EXIT', {badarg, _}} = (catch ets:update_counter(T, key, Ops)),
    [{key, 10, not_a_counter}] = ets:lookup(T, key),
    ok.
