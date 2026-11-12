%% Erlang stdlib: maps module
%%
%% This module provides the standard Erlang maps API. It is split into
%% two layers:
%%
%% BIF functions (implemented in Rust FFI — nera-runtime/src/ffi.rs):
%%   new/0, get/2, get/3, find/2, put/3, remove/2, is_key/2,
%%   keys/1, values/1, to_list/1, from_list/1, from_keys/2,
%%   size/1, merge/2, update/3, take/2
%%   These are dispatched directly by the compiler via the BIF table
%%   and do NOT need Erlang definitions here.
%%
%% Erlang functions (implemented below):
%%   Higher-order functions (map, filter, fold, etc.) are written in
%%   Erlang so they can call user-supplied funs with proper TCO and
%%   yield semantics.
-module(maps).
-export([iterator/1, next/1, map/2, filter/2, filtermap/2, fold/3, foreach/2,
         with/2, without/2,
         update_with/3, update_with/4,
         groups_from_list/2, groups_from_list/3,
         intersect/2, intersect_with/3]).

%% iterator(Map) -> Iterator
%% An iterator is `none` or `{Key, Value, NextIterator}`, which maps:next/1
%% also accepts from OTP. A non-map raises {badmap, Map}.
iterator(M) when is_map(M) ->
    iterator_1(maps:to_list(M));
iterator(M) ->
    error({badmap, M}).

iterator_1([]) -> none;
iterator_1([{K, V} | T]) -> {K, V, iterator_1(T)}.

%% next(Iterator) -> {Key, Value, NextIterator} | none
%% Anything but an iterator raises badarg.
next({K, V, I}) -> {K, V, I};
next(none) -> none;
next(_) -> error(badarg).

%% map(Fun, Map) -> Map2
%% Fun = fun((Key, Value1) -> Value2)
%% Returns a map with the same keys, with each value replaced by Fun(K, V).
map(F, M) ->
    maps:from_list(map_1(F, maps:to_list(M))).

map_1(_F, []) -> [];
map_1(F, [{K, V} | T]) -> [{K, F(K, V)} | map_1(F, T)].

%% filter(Pred, Map) -> Map2
%% Pred = fun((Key, Value) -> boolean())
%% Returns a map containing only entries for which Pred(K, V) is true.
filter(Pred, M) ->
    maps:from_list(filter_1(Pred, maps:to_list(M))).

filter_1(_Pred, []) -> [];
filter_1(Pred, [{K, V} | T]) ->
    case Pred(K, V) of
        true -> [{K, V} | filter_1(Pred, T)];
        false -> filter_1(Pred, T)
    end.

%% filtermap(Fun, Map) -> Map2
%% Fun = fun((Key, Value) -> true | {true, NewValue} | false)
%% Combined filter and map in one pass.
filtermap(Fun, M) ->
    maps:from_list(filtermap_1(Fun, maps:to_list(M))).

filtermap_1(_Fun, []) -> [];
filtermap_1(Fun, [{K, V} | T]) ->
    case Fun(K, V) of
        true -> [{K, V} | filtermap_1(Fun, T)];
        {true, NewV} -> [{K, NewV} | filtermap_1(Fun, T)];
        false -> filtermap_1(Fun, T)
    end.

%% fold(Fun, Init, Map) -> Acc
%% Fun = fun((Key, Value, AccIn) -> AccOut)
%% Folds over all entries in the map.
fold(F, Init, M) ->
    fold_1(F, Init, maps:to_list(M)).

fold_1(_F, Acc, []) -> Acc;
fold_1(F, Acc, [{K, V} | T]) -> fold_1(F, F(K, V, Acc), T).

%% foreach(Fun, Map) -> ok
%% Fun = fun((Key, Value) -> term())
%% Applies Fun to each entry for side effects.
foreach(F, M) ->
    foreach_1(F, maps:to_list(M)).

foreach_1(_F, []) -> ok;
foreach_1(F, [{K, V} | T]) -> F(K, V), foreach_1(F, T).

%% with(Keys, Map) -> Map2
%% Returns a map containing only the keys in Keys that exist in Map.
with(Ks, M) ->
    with_1(Ks, M, maps:new()).

with_1([], _M, Acc) -> Acc;
with_1([K | T], M, Acc) ->
    case maps:find(K, M) of
        {ok, V} -> with_1(T, M, maps:put(K, V, Acc));
        error -> with_1(T, M, Acc)
    end.

%% without(Keys, Map) -> Map2
%% Returns the map with all given keys removed.
without(Ks, M) ->
    without_1(Ks, M).

without_1([], M) -> M;
without_1([K | T], M) -> without_1(T, maps:remove(K, M)).

%% update_with(Key, Fun, Map) -> Map2
%% Fun = fun((Value1) -> Value2)
%% Updates the value for an existing key using Fun. Raises {badkey, Key} if missing.
update_with(Key, Fun, M) ->
    case maps:find(Key, M) of
        {ok, V} -> maps:put(Key, Fun(V), M);
        error -> error({badkey, Key})
    end.

%% update_with(Key, Fun, Init, Map) -> Map2
%% Like update_with/3, but uses Init if the key is missing.
update_with(Key, Fun, Init, M) ->
    case maps:find(Key, M) of
        {ok, V} -> maps:put(Key, Fun(V), M);
        error -> maps:put(Key, Init, M)
    end.

%% groups_from_list(KeyFun, List) -> MapOfLists
%% KeyFun = fun((Elem) -> Key)
%% Groups elements by the result of KeyFun. Values are in input order.
groups_from_list(KeyFun, List) ->
    groups_from_list(KeyFun, fun(_Elem) -> _Elem end, List).

%% groups_from_list(KeyFun, ValueFun, List) -> MapOfLists
%% Groups elements, transforming each via ValueFun.
groups_from_list(KeyFun, ValueFun, List) ->
    groups_build(KeyFun, ValueFun, List, maps:new()).

groups_build(_KeyFun, _ValueFun, [], Acc) ->
    %% Reverse all accumulated lists to restore input order
    maps:map(fun(_K, V) -> lists:reverse(V) end, Acc);
groups_build(KeyFun, ValueFun, [H | T], Acc) ->
    K = KeyFun(H),
    V = ValueFun(H),
    NewAcc = case maps:find(K, Acc) of
        {ok, Existing} -> maps:put(K, [V | Existing], Acc);
        error -> maps:put(K, [V], Acc)
    end,
    groups_build(KeyFun, ValueFun, T, NewAcc).

%% intersect(Map1, Map2) -> Map3
%% Returns a map with keys common to both maps. Values from Map2 win.
intersect(M1, M2) ->
    intersect_with(fun(_K, _V1, V2) -> V2 end, M1, M2).

%% intersect_with(Combiner, Map1, Map2) -> Map3
%% Combiner = fun((Key, Value1, Value2) -> Value3)
%% Returns a map with keys common to both, combining values with Combiner.
intersect_with(Combiner, M1, M2) ->
    intersect_build(Combiner, maps:to_list(M1), M2, maps:new()).

intersect_build(_Combiner, [], _M2, Acc) -> Acc;
intersect_build(Combiner, [{K, V1} | T], M2, Acc) ->
    case maps:find(K, M2) of
        {ok, V2} -> intersect_build(Combiner, T, M2, maps:put(K, Combiner(K, V1, V2), Acc));
        error -> intersect_build(Combiner, T, M2, Acc)
    end.
