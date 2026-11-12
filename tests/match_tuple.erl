%% Adapted from OTP compiler/test/match_SUITE.erl (Apache 2.0)
%% Tests tuple pattern matching, including nested tuples and arity discrimination.
-module(match_tuple).
-export([test/0]).

test() ->
    %% Different arities
    one = by_arity({a}),
    two = by_arity({a, b}),
    three = by_arity({a, b, c}),

    %% First-element discrimination
    x_tuple = by_tag({x, 1, 2}),
    y_tuple = by_tag({y, 3}),
    z_tuple = by_tag({z}),

    %% Nested tuple matching
    {inner, 42} = nested_tuple({outer, {inner, 42}}),

    %% Tuple in case
    {ok, 5} = tuple_case({add, 2, 3}),
    {ok, 6} = tuple_case({mul, 2, 3}),
    error = tuple_case({unknown, 1}),

    %% Deep nesting
    deep = deep_match({a, {b, {c, {d, deep}}}}),

    %% Tuple with literal elements
    ok = exact({error, not_found}),
    ok = exact({ok, done}),
    other = exact({ok, pending}),

    %% Tuple construction and matching roundtrip
    T = {1, 2, 3},
    {1, 2, 3} = T,
    1 = element(1, T),
    2 = element(2, T),
    3 = element(3, T),
    3 = tuple_size(T),

    io:format("ok~n"),
    ok.

by_arity({_}) -> one;
by_arity({_, _}) -> two;
by_arity({_, _, _}) -> three.

by_tag({x, _, _}) -> x_tuple;
by_tag({y, _}) -> y_tuple;
by_tag({z}) -> z_tuple.

nested_tuple({outer, Inner}) -> Inner.

tuple_case(T) ->
    case T of
        {add, A, B} -> {ok, A + B};
        {mul, A, B} -> {ok, A * B};
        _ -> error
    end.

deep_match({a, {b, {c, {d, X}}}}) -> X.

exact({error, not_found}) -> ok;
exact({ok, done}) -> ok;
exact(_) -> other.
