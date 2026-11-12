%% Erlang stdlib: lists module
%%
%% This module provides the standard Erlang lists API. It is split into
%% two layers:
%%
%% BIF functions (implemented in Rust FFI — nera-runtime/src/ffi.rs):
%%   reverse/1, reverse/2, member/2, keyfind/3, keymember/3,
%%   keysearch/3, append/1, last/1, nth/2
%%   These are dispatched directly by the compiler via the BIF table
%%   and do NOT need Erlang definitions here.
%%
%% Erlang functions (implemented below):
%%   Higher-order functions (map, filter, foldl, etc.) and recursive
%%   algorithms (sort, flatten, zip, etc.) are written in Erlang so
%%   they can call user-supplied funs with proper TCO and yield semantics.
-module(lists).
-export([map/2, filter/2, filtermap/2, foldl/3, foldr/3, foreach/2,
         all/2, any/2, flatmap/2, partition/2,
         takewhile/2, dropwhile/2,
         sort/1, sort/2,
         flatten/1, flatten/2,
         zip/2, unzip/1,
         seq/2, seq/3,
         duplicate/2,
         append/2, subtract/2, join/2, suffix/2,
         keytake/3, keydelete/3, keyreplace/4, keystore/4,
         ukeymerge/3,
         usort/1,
         max/1, min/1,
         sum/1,
         split/2, sublist/2,
         splitwith/2,
         mapfoldl/3]).

%% map(Fun, List) -> [Fun(Elem) || Elem <- List]
%% Tail-recursive with accumulator to avoid stack overflow on large lists.
map(F, L) -> map(F, L, []).
map(_F, [], Acc) -> lists:reverse(Acc);
map(F, [H|T], Acc) -> map(F, T, [F(H)|Acc]).

%% filter(Pred, List) -> [Elem || Elem <- List, Pred(Elem)]
%% Tail-recursive with accumulator.
filter(Pred, L) -> filter(Pred, L, []).
filter(_Pred, [], Acc) -> lists:reverse(Acc);
filter(Pred, [H|T], Acc) ->
    case Pred(H) of
        true -> filter(Pred, T, [H|Acc]);
        false -> filter(Pred, T, Acc)
    end.

%% foldl(Fun, Acc, List) -> Fun(Elem, Acc) applied left-to-right
foldl(_F, Acc, []) -> Acc;
foldl(F, Acc, [H|T]) -> foldl(F, F(H, Acc), T).

%% foldr(Fun, Acc, List) -> Fun(Elem, Acc) applied right-to-left
foldr(_F, Acc, []) -> Acc;
foldr(F, Acc, [H|T]) -> F(H, foldr(F, Acc, T)).

%% foreach(Fun, List) -> ok. Apply Fun to each element for side effects.
foreach(_F, []) -> ok;
foreach(F, [H|T]) -> F(H), foreach(F, T).

%% all(Pred, List) -> true if Pred(Elem) is true for all elements.
all(_Pred, []) -> true;
all(Pred, [H|T]) ->
    case Pred(H) of
        true -> all(Pred, T);
        false -> false
    end.

%% any(Pred, List) -> true if Pred(Elem) is true for any element.
any(_Pred, []) -> false;
any(Pred, [H|T]) ->
    case Pred(H) of
        true -> true;
        false -> any(Pred, T)
    end.

%% flatmap(Fun, List) -> append([Fun(Elem) || Elem <- List])
%% Tail-recursive with accumulator.
flatmap(F, L) -> flatmap(F, L, []).
flatmap(_F, [], Acc) -> lists:reverse(Acc);
flatmap(F, [H|T], Acc) ->
    flatmap(F, T, rev_prepend(F(H), Acc)).

rev_prepend([], Acc) -> Acc;
rev_prepend([H|T], Acc) -> rev_prepend(T, [H|Acc]).

%% partition(Pred, List) -> {Satisfying, NotSatisfying}
partition(Pred, L) -> partition(Pred, L, [], []).

partition(_Pred, [], Yes, No) ->
    {lists:reverse(Yes), lists:reverse(No)};
partition(Pred, [H|T], Yes, No) ->
    case Pred(H) of
        true -> partition(Pred, T, [H|Yes], No);
        false -> partition(Pred, T, Yes, [H|No])
    end.

%% takewhile(Pred, List) -> longest prefix where Pred is true
%% Tail-recursive with accumulator.
takewhile(Pred, L) -> takewhile(Pred, L, []).
takewhile(_Pred, [], Acc) -> lists:reverse(Acc);
takewhile(Pred, [H|T], Acc) ->
    case Pred(H) of
        true -> takewhile(Pred, T, [H|Acc]);
        false -> lists:reverse(Acc)
    end.

%% dropwhile(Pred, List) -> remainder after longest prefix where Pred is true
dropwhile(_Pred, []) -> [];
dropwhile(Pred, [H|T] = L) ->
    case Pred(H) of
        true -> dropwhile(Pred, T);
        false -> L
    end.

%% sort/1 — merge sort
sort([]) -> [];
sort([X]) -> [X];
sort(L) ->
    {A, B} = split_halves(L),
    merge(sort(A), sort(B)).

%% sort/2 — merge sort with custom comparison fun
sort(_Fun, []) -> [];
sort(_Fun, [X]) -> [X];
sort(Fun, L) ->
    {A, B} = split_halves(L),
    merge_fun(Fun, sort(Fun, A), sort(Fun, B)).

%% flatten/1 — flatten arbitrarily nested lists
%% Uses explicit continuation stack to avoid stack overflow on long lists.
flatten(L) -> flatten(L, []).

%% flatten/2 — flatten with tail
flatten(L, Tail) -> do_flatten(L, [Tail], []).

do_flatten([], [], Acc) -> lists:reverse(Acc);
do_flatten([], [Cont|Stack], Acc) -> do_flatten(Cont, Stack, Acc);
do_flatten([H|T], Stack, Acc) when is_list(H) ->
    do_flatten(H, [T|Stack], Acc);
do_flatten([H|T], Stack, Acc) ->
    do_flatten(T, Stack, [H|Acc]).

%% zip/2 — combine two lists into a list of pairs
%% Tail-recursive with accumulator.
zip(L1, L2) -> zip(L1, L2, []).
zip([], [], Acc) -> lists:reverse(Acc);
zip([H1|T1], [H2|T2], Acc) -> zip(T1, T2, [{H1, H2}|Acc]).

%% unzip/1 — split a list of pairs into two lists
unzip(L) -> unzip(L, [], []).

unzip([], As, Bs) ->
    {lists:reverse(As), lists:reverse(Bs)};
unzip([{A, B}|T], As, Bs) ->
    unzip(T, [A|As], [B|Bs]).

%% seq/2,3 — generate a sequence of integers (tail-recursive)
seq(From, To) -> seq(From, To, 1).

seq(From, To, Step) when Step > 0, From > To -> [];
seq(From, To, Step) when Step < 0, From < To -> [];
seq(From, To, Step) ->
    seq_loop(From, To, Step, []).

seq_loop(From, To, Step, Acc) when Step > 0, From > To -> lists:reverse(Acc);
seq_loop(From, To, Step, Acc) when Step < 0, From < To -> lists:reverse(Acc);
seq_loop(From, To, Step, Acc) ->
    seq_loop(From + Step, To, Step, [From | Acc]).

%% duplicate/2 — make N copies of an element (tail-recursive, matches OTP)
duplicate(N, Elem) when is_integer(N), N >= 0 -> duplicate(N, Elem, []).
duplicate(0, _Elem, Acc) -> Acc;
duplicate(N, Elem, Acc) when N > 0 -> duplicate(N - 1, Elem, [Elem | Acc]).

%% join/2 — insert the separator as one element without flattening it.
join(_Separator, []) -> [];
join(Separator, [First|Rest]) -> join(Separator, Rest, [First]).

join(_Separator, [], Acc) -> lists:reverse(Acc);
join(Separator, [Head|Tail], Acc) ->
    join(Separator, Tail, [Head, Separator|Acc]).

%% append/2 — same as ++
append(L1, L2) -> L1 ++ L2.

%% subtract/2 — same as --
subtract(L1, L2) -> L1 -- L2.

%% keytake/3 — remove the first tuple whose Nth element equals Key.
%% A guard skips non-tuples and short tuples; == preserves numeric equality.
keytake(Key, N, List) when is_integer(N), N > 0 ->
    keytake_loop(Key, N, List, []).

keytake_loop(_Key, _N, [], _Prefix) -> false;
keytake_loop(Key, N, [H|T], Prefix) when element(N, H) == Key ->
    {value, H, lists:reverse(Prefix, T)};
keytake_loop(Key, N, [H|T], Prefix) ->
    keytake_loop(Key, N, T, [H|Prefix]).

%% keydelete/3 — delete the first tuple where element N matches Key.
%% A guard skips non-tuples and short tuples; == preserves numeric equality.
%% Tail-recursive with accumulator.
keydelete(Key, N, L) when is_integer(N), N > 0 -> keydelete(Key, N, L, []).
keydelete(_Key, _N, [], Acc) -> lists:reverse(Acc);
keydelete(Key, N, [H|T], Acc) when element(N, H) == Key ->
    lists:reverse(Acc) ++ T;
keydelete(Key, N, [H|T], Acc) ->
    keydelete(Key, N, T, [H|Acc]).

%% keyreplace/4 — replace the first tuple where element N matches Key.
%% As in OTP, the entry takes only a positive integer position and a tuple
%% replacement; anything else raises function_clause.
%% A guard skips non-tuples and short tuples; == preserves numeric equality.
%% Tail-recursive with accumulator.
keyreplace(Key, N, L, NewTuple) when is_integer(N), N > 0, is_tuple(NewTuple) ->
    keyreplace(Key, N, L, NewTuple, []).
keyreplace(_Key, _N, [], _NewTuple, Acc) -> lists:reverse(Acc);
keyreplace(Key, N, [H|T], NewTuple, Acc) when element(N, H) == Key ->
    lists:reverse(Acc) ++ [NewTuple | T];
keyreplace(Key, N, [H|T], NewTuple, Acc) ->
    keyreplace(Key, N, T, NewTuple, [H|Acc]).

%% keystore/4 — like keyreplace but appends if not found.
%% As in OTP, the entry takes only a positive integer position and a tuple
%% replacement; anything else raises function_clause.
%% A guard skips non-tuples and short tuples; == preserves numeric equality.
%% Tail-recursive with accumulator.
keystore(Key, N, L, NewTuple) when is_integer(N), N > 0, is_tuple(NewTuple) ->
    keystore(Key, N, L, NewTuple, []).
keystore(_Key, _N, [], NewTuple, Acc) -> lists:reverse([NewTuple|Acc]);
keystore(Key, N, [H|T], NewTuple, Acc) when element(N, H) == Key ->
    lists:reverse(Acc) ++ [NewTuple | T];
keystore(Key, N, [H|T], NewTuple, Acc) ->
    keystore(Key, N, T, NewTuple, [H|Acc]).

%% ukeymerge/3 — merge two key-sorted lists, removing duplicates by key position
%% Tail-recursive with accumulator.
ukeymerge(N, L1, L2) -> ukeymerge(N, L1, L2, []).
ukeymerge(_N, [], L2, Acc) -> lists:reverse(Acc) ++ L2;
ukeymerge(_N, L1, [], Acc) -> lists:reverse(Acc) ++ L1;
ukeymerge(N, [H1|T1] = L1, [H2|T2] = L2, Acc) ->
    case element(N, H1) < element(N, H2) of
        true -> ukeymerge(N, T1, L2, [H1|Acc]);
        false ->
            case element(N, H1) > element(N, H2) of
                true -> ukeymerge(N, L1, T2, [H2|Acc]);
                false -> ukeymerge(N, T1, T2, [H1|Acc])
            end
    end.

%% usort/1 — sort and remove duplicates
usort([]) -> [];
usort(L) -> usort_dedup(sort(L)).

usort_dedup(L) -> usort_dedup(L, []).
usort_dedup([], Acc) -> lists:reverse(Acc);
usort_dedup([X], Acc) -> lists:reverse([X|Acc]);
usort_dedup([X, X | T], Acc) -> usort_dedup([X | T], Acc);
usort_dedup([X, Y | T], Acc) -> usort_dedup([Y | T], [X|Acc]).

%% max/1 — maximum element of a non-empty list
max([H|T]) -> max(T, H).
max([], Max) -> Max;
max([H|T], Max) when H > Max -> max(T, H);
max([_|T], Max) -> max(T, Max).

%% min/1 — minimum element of a non-empty list
min([H|T]) -> min(T, H).
min([], Min) -> Min;
min([H|T], Min) when H < Min -> min(T, H);
min([_|T], Min) -> min(T, Min).

%% sum/1 — sum of a list of numbers
sum(L) -> foldl(fun(X, Acc) -> X + Acc end, 0, L).

%% filtermap/2 — combined filter and map.
%% Fun returns false (drop), true (keep as-is), or {true, NewValue} (replace).
filtermap(F, L) -> filtermap(F, L, []).
filtermap(_F, [], Acc) -> lists:reverse(Acc);
filtermap(F, [H|T], Acc) ->
    case F(H) of
        true -> filtermap(F, T, [H|Acc]);
        {true, V} -> filtermap(F, T, [V|Acc]);
        false -> filtermap(F, T, Acc)
    end.

%% sublist/2 — copy at most Count elements, without inspecting the unused tail.
sublist(List, Count) when is_list(List), is_integer(Count), Count >= 0 ->
    sublist_prefix(Count, List, []).

sublist_prefix(0, _Rest, Acc) -> lists:reverse(Acc);
sublist_prefix(_Count, [], Acc) -> lists:reverse(Acc);
sublist_prefix(Count, [Head|Tail], Acc) ->
    sublist_prefix(Count - 1, Tail, [Head|Acc]).

%% split/2 — split a list into the first N elements and the rest.
%% Errors with badarg if N is negative or larger than length(L).
split(N, L) when is_integer(N), N >= 0 -> split(N, L, []).
split(0, L, Acc) -> {lists:reverse(Acc), L};
split(_N, [], _Acc) -> erlang:error(badarg);
split(N, [H|T], Acc) -> split(N - 1, T, [H|Acc]).

%% splitwith/2 — split list at the point where Pred first returns false
splitwith(Pred, L) -> splitwith(Pred, L, []).

splitwith(_Pred, [], Acc) ->
    {lists:reverse(Acc), []};
splitwith(Pred, [H|T] = L, Acc) ->
    case Pred(H) of
        true -> splitwith(Pred, T, [H|Acc]);
        false -> {lists:reverse(Acc), L}
    end.

%% mapfoldl/3 — combined map and foldl (tail-recursive)
mapfoldl(F, Acc, L) -> mapfoldl(F, Acc, L, []).
mapfoldl(_F, Acc, [], MappedAcc) -> {lists:reverse(MappedAcc), Acc};
mapfoldl(F, Acc, [H|T], MappedAcc) ->
    {MappedH, Acc1} = F(H, Acc),
    mapfoldl(F, Acc1, T, [MappedH|MappedAcc]).

%% --- internal helpers ---

%% Split a list into two roughly equal halves (internal merge-sort helper).
%% Renamed from `split` to avoid colliding with the public `lists:split/2`
%% helper `split/3` — both would otherwise land under the same name/arity.
split_halves(L) -> split_halves(L, [], []).
split_halves([], A, B) -> {A, B};
split_halves([H], A, B) -> {[H|A], B};
split_halves([H1, H2|T], A, B) -> split_halves(T, [H1|A], [H2|B]).

%% Merge two sorted lists (for sort/1) — tail-recursive
merge(A, B) -> merge(A, B, []).
merge([], B, Acc) -> lists:reverse(Acc) ++ B;
merge(A, [], Acc) -> lists:reverse(Acc) ++ A;
merge([HA|TA] = A, [HB|TB] = B, Acc) ->
    case HA =< HB of
        true -> merge(TA, B, [HA|Acc]);
        false -> merge(A, TB, [HB|Acc])
    end.

%% Merge two sorted lists with custom comparison (for sort/2) — tail-recursive
merge_fun(Fun, A, B) -> merge_fun(Fun, A, B, []).
merge_fun(_Fun, [], B, Acc) -> lists:reverse(Acc) ++ B;
merge_fun(_Fun, A, [], Acc) -> lists:reverse(Acc) ++ A;
merge_fun(Fun, [HA|TA] = A, [HB|TB] = B, Acc) ->
    case Fun(HA, HB) of
        true -> merge_fun(Fun, TA, B, [HA|Acc]);
        false -> merge_fun(Fun, A, TB, [HB|Acc])
    end.

%% Compare a suffix with exact term equality; validate both lists first.
suffix(Suffix, List) ->
    SuffixLength = length(Suffix),
    ListLength = length(List),
    case SuffixLength =< ListLength of
        true -> suffix_drop(ListLength - SuffixLength, List) =:= Suffix;
        false -> false
    end.
suffix_drop(0, List) -> List;
suffix_drop(N, [_ | Rest]) -> suffix_drop(N - 1, Rest).
