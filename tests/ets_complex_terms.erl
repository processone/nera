%% Tests ETS insert/lookup with complex term types.
%% Phase 5 regression baseline: exercises deep_copy_into_compact and
%% deep_copy_into_heap across all boxed subtypes.
-module(ets_complex_terms).
-export([test/0]).

test() ->
    T = ets:new(complex, [set, public]),
    test_list_value(T),
    test_float_value(T),
    test_small_binary(T),
    test_large_binary(T),
    test_deep_nesting(T),
    test_multiple_complex(T),
    ets:delete(T),
    ok.

id(X) -> X.

test_list_value(T) ->
    ets:insert(T, {key1, [1, 2, 3]}),
    [{key1, [1, 2, 3]}] = ets:lookup(T, key1),
    ok.

test_float_value(T) ->
    F = id(3.14),
    ets:insert(T, {key3, F}),
    [{key3, Val}] = ets:lookup(T, key3),
    true = is_float(Val),
    true = (Val > 3.13 andalso Val < 3.15),
    ok.

test_small_binary(T) ->
    ets:insert(T, {key4, <<"hello">>}),
    [{key4, <<"hello">>}] = ets:lookup(T, key4),
    ok.

test_large_binary(T) ->
    %% >64 bytes triggers refc binary
    BigBin = list_to_binary(lists:duplicate(100, 42)),
    ets:insert(T, {key5, BigBin}),
    [{key5, LB}] = ets:lookup(T, key5),
    100 = byte_size(LB),
    ok.

test_deep_nesting(T) ->
    %% 5 levels: outer tuple > inner tuple > list > deeper tuple > float
    Inner = {deep, [1, {bottom, id(2.0)}]},
    ets:insert(T, {key6, {nested, Inner}}),
    [{key6, {nested, {deep, [1, {bottom, Xv}]}}}] = ets:lookup(T, key6),
    true = is_float(Xv),
    true = (Xv > 1.99 andalso Xv < 2.01),
    ok.

test_multiple_complex(T) ->
    %% Tuple with mix of types
    ets:insert(T, {key8, [1, 2], <<"bin">>, id(1.5), {inner, ok}}),
    [{key8, [1, 2], <<"bin">>, Fv, {inner, ok}}] = ets:lookup(T, key8),
    true = (Fv > 1.49 andalso Fv < 1.51),
    ok.
