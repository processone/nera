%% Adapted from OTP compiler/test/guard_SUITE.erl (Apache 2.0)
%% Tests is_boolean, is_function/2, and size/1 in guards.
-module(guard_type_tests).
-export([test/0]).

test() ->
    %% is_boolean/1 in guards
    true = is_bool_guard(true),
    true = is_bool_guard(false),
    false = is_bool_guard(hello),
    false = is_bool_guard(42),
    false = is_bool_guard(0),
    false = is_bool_guard(1),
    false = is_bool_guard([]),
    false = is_bool_guard({true}),

    %% is_boolean with variable (prevent constant folding)
    true = is_bool_guard(id(true)),
    true = is_bool_guard(id(false)),
    false = is_bool_guard(id(hello)),
    false = is_bool_guard(id(42)),

    %% is_function/1 in guards
    true = is_fun_guard(fun() -> ok end),
    false = is_fun_guard(hello),
    false = is_fun_guard(42),

    %% is_function/2 in guards (correct arity)
    F0 = fun() -> ok end,
    F1 = fun(X) -> X end,
    F2 = fun(X, Y) -> {X, Y} end,
    true = is_fun2_guard(F0, 0),
    false = is_fun2_guard(F0, 1),
    true = is_fun2_guard(F1, 1),
    false = is_fun2_guard(F1, 0),
    false = is_fun2_guard(F1, 2),
    true = is_fun2_guard(F2, 2),
    false = is_fun2_guard(F2, 1),
    false = is_fun2_guard(hello, 0),

    %% size/1 in guards with tuples
    true = size_guard({a,b,c}, 3),
    false = size_guard({a,b}, 3),
    true = size_guard({}, 0),

    %% size/1 in guards with binaries
    true = size_guard(<<1,2,3>>, 3),
    false = size_guard(<<1,2>>, 3),
    true = size_guard(<<>>, 0),

    %% tuple_size/1 specifically
    true = tuple_size_guard({a,b}, 2),
    true = tuple_size_guard({a,b,c}, 3),
    false = tuple_size_guard({a}, 2),

    %% Combined type tests in guards
    ok = combined_type(id(true), id({a,b,c})),
    ok = combined_type(id(false), id({a,b,c})),
    error = combined_type(id(hello), id({a,b,c})),
    error = combined_type(id(true), id({a,b})),

    io:format("ok~n"),
    ok.

is_bool_guard(X) when is_boolean(X) -> true;
is_bool_guard(_) -> false.

is_fun_guard(X) when is_function(X) -> true;
is_fun_guard(_) -> false.

is_fun2_guard(F, Arity) when is_function(F, Arity) -> true;
is_fun2_guard(_, _) -> false.

size_guard(X, N) when size(X) =:= N -> true;
size_guard(_, _) -> false.

tuple_size_guard(T, N) when tuple_size(T) =:= N -> true;
tuple_size_guard(_, _) -> false.

combined_type(B, T) when is_boolean(B), tuple_size(T) =:= 3 -> ok;
combined_type(_, _) -> error.

id(I) -> I.
