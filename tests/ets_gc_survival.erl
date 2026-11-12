%% Tests that ETS lookup results survive garbage collection.
%% Phase 5 regression baseline: after zero-copy, external pointers
%% must be preserved by the GC (not relocated or forwarded).
-module(ets_gc_survival).
-export([test/0]).

test() ->
    T = ets:new(gc_test, [set, public]),

    %% Insert complex terms
    ets:insert(T, {key1, [1, 2, 3, 4, 5]}),
    ets:insert(T, {key2, {nested, {deep, [10, 20, 30]}}}),
    ets:insert(T, {key3, list_to_binary(lists:duplicate(100, 65))}),

    %% Lookup all three
    [{key1, List1}] = ets:lookup(T, key1),
    [{key2, Nested}] = ets:lookup(T, key2),
    [{key3, Bin}] = ets:lookup(T, key3),

    %% Allocate garbage to trigger GC (20K tuples ~= 1.7MB)
    _ = [erlang:make_tuple(10, I) || I <- lists:seq(1, 20000)],

    %% Verify all lookup results survived GC
    [1, 2, 3, 4, 5] = List1,
    5 = length(List1),

    {nested, {deep, [10, 20, 30]}} = Nested,

    100 = byte_size(Bin),

    ets:delete(T),
    ok.
