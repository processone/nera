%% EXPECT: ets_structural_keys_ok
%% Hash-backed ETS tables use exact term identity, not allocation addresses.
%% Run the same assertions natively and on BEAM.
-module(ets_structural_keys).
-export([test/0]).

test() ->
    N = id(3),
    [check_table(Type, N) || Type <- [set, bag, duplicate_bag]],
    io:format("ets_structural_keys_ok~n"),
    ok.

id(X) -> X.

map_key(N, forward) ->
    maps:from_list([{N, integer}, {N / 1, float}, {items, [N]}]);
map_key(N, reverse) ->
    maps:from_list([{items, [N]}, {N / 1, float}, {N, integer}]).

float_key(N) -> N / 2.

bigint_key(N) -> (1 bsl 80) + N.

check_table(Type, N) ->
    T = ets:new(structural_keys, [Type]),
    Map = map_key(N, forward),
    check_lookup(T, Map, map_key(N, reverse), Map#{N => changed}),
    check_lookup(T, float_key(N), float_key(N), float_key(N + 1)),
    check_lookup(T, bigint_key(N), bigint_key(N), -bigint_key(N)),
    check_lookup(T, {wrapped, Map}, {wrapped, map_key(N, reverse)}, {wrapped, Map#{items => [N + 1]}}),
    check_lookup(T, [float_key(N)], [float_key(N)], [float_key(N + 1)]),
    check_lookup(T, #{value => -bigint_key(N)}, #{value => -bigint_key(N)}, #{value => -bigint_key(N + 1)}),
    %% Numerically equal keys need not be exact matches, even inside a map.
    check_lookup(T, N / 1, N / 1, N),
    check_lookup(T, #{value => N / 1}, #{value => N / 1}, #{value => N}),
    true = ets:delete(T).

check_lookup(T, Key, Equal, Different) ->
    Row = {Key, stored},
    true = ets:insert(T, Row),
    [Row] = ets:lookup(T, Equal),
    [] = ets:lookup(T, Different),
    true = ets:delete(T, Key).
