%% Adapted from OTP compiler/test/guard_SUITE.erl (Apache 2.0)
%% Tests BIF calls in guard expressions.
-module(guard_bifs).
-export([test/0]).

test() ->
    %% abs in guard
    ok = check_abs(-5, 5),
    ok = check_abs(5, 5),
    fail = check_abs(-5, 4),

    %% length in guard
    short = len_class([1]),
    short = len_class([1, 2]),
    medium = len_class([1, 2, 3]),
    medium = len_class([1, 2, 3, 4, 5]),
    long = len_class([1, 2, 3, 4, 5, 6]),

    %% tuple_size in guard
    small_tuple = tuple_class({a}),
    small_tuple = tuple_class({a, b}),
    big_tuple = tuple_class({a, b, c}),
    big_tuple = tuple_class({a, b, c, d}),

    %% element in guard
    ok = check_first({ok, data}),
    error = check_first({error, reason}),
    other = check_first({warning, msg}),

    %% hd and tl in guard
    ok = check_hd([ok, 1, 2]),
    error = check_hd([error, 1, 2]),
    other = check_hd([]),

    %% byte_size in guard
    empty = bin_class(<<>>),
    small_bin = bin_class(<<1>>),
    small_bin = bin_class(<<1, 2, 3>>),
    big_bin = bin_class(<<1, 2, 3, 4, 5>>),

    %% map_size in guard
    empty_map = map_class(#{}),
    small_map = map_class(#{a => 1}),
    small_map = map_class(#{a => 1, b => 2}),
    big_map = map_class(#{a => 1, b => 2, c => 3}),

    %% Combining multiple BIF calls in guard
    ok = complex_guard({ok, [1, 2, 3]}, 3),
    fail = complex_guard({ok, [1, 2]}, 3),
    fail = complex_guard({error, [1, 2, 3]}, 3),

    io:format("ok~n"),
    ok.

check_abs(X, Expected) when abs(X) == Expected -> ok;
check_abs(_, _) -> fail.

len_class(L) when length(L) =< 2 -> short;
len_class(L) when length(L) =< 5 -> medium;
len_class(_) -> long.

tuple_class(T) when tuple_size(T) =< 2 -> small_tuple;
tuple_class(_) -> big_tuple.

check_first(T) when element(1, T) == ok -> ok;
check_first(T) when element(1, T) == error -> error;
check_first(_) -> other.

check_hd(L) when hd(L) == ok -> ok;
check_hd(L) when hd(L) == error -> error;
check_hd(_) -> other.

bin_class(B) when byte_size(B) == 0 -> empty;
bin_class(B) when byte_size(B) =< 4 -> small_bin;
bin_class(_) -> big_bin.

map_class(M) when map_size(M) == 0 -> empty_map;
map_class(M) when map_size(M) =< 2 -> small_map;
map_class(_) -> big_map.

complex_guard(T, N) when element(1, T) == ok, length(element(2, T)) == N -> ok;
complex_guard(_, _) -> fail.
