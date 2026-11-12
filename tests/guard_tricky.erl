%% Adapted from OTP compiler/test/guard_SUITE.erl (Apache 2.0)
%% Tests tricky guard edge cases, constant conditions, and guard failure behavior.
-module(guard_tricky).
-export([test/0]).

test() ->
    %% Guards that always fail (dead code)
    no = always_fail(42),
    no = always_fail(hello),
    no = always_fail(true),

    %% Constant conditions
    yes = const_true(),
    no = const_false(),

    %% tuple_size edge cases
    ok = tuple_size_edge({a,b,c}, 3),
    error = tuple_size_edge({a,b,c}, 1000000),
    error = tuple_size_edge({a,b,c}, -1),
    error = tuple_size_edge(not_a_tuple, 3),

    %% Chained element/andalso patterns
    ok = chain_elem({tag, 42, hello}),
    error = chain_elem({tag, 42, 42}),
    error = chain_elem({other, 42, hello}),
    error = chain_elem({tag}),
    error = chain_elem(not_a_tuple),

    %% Guard exceptions caught as false
    no = guard_exception(not_a_tuple),
    no = guard_exception([]),
    yes = guard_exception({a,b,c}),

    %% Tricky: guard with arithmetic that may throw
    ok = guard_arith(10, 2),
    error = guard_arith(10, 0),
    error = guard_arith(hello, 2),

    %% andalso/orelse with exceptions in second operand
    false = andalso_exc(id(false), id(not_a_tuple)),
    true = orelse_exc(id(true), id(not_a_tuple)),

    %% Guard with multiple failing BIF calls
    a = multi_bif_fail(id({a,b,c})),
    b = multi_bif_fail(id([1,2,3])),
    c = multi_bif_fail(id(other)),

    %% Tricky boolean expressions in guards
    true = tricky_bool(id(true), id(true)),
    false = tricky_bool(id(true), id(false)),
    false = tricky_bool(id(false), id(true)),
    false = tricky_bool(id(false), id(false)),

    %% Guard with nested tuple access
    ok = nested_tuple({{a, 1}, {b, 2}}),
    error = nested_tuple({{a, 1}, {b, 3}}),
    error = nested_tuple(not_a_tuple),

    io:format("ok~n"),
    ok.

%% Guard that always fails: is_integer and is_atom can't both be true
always_fail(X) when is_integer(X), is_atom(X) -> yes;
always_fail(_) -> no.

const_true() ->
    if
        true -> yes
    end.

const_false() ->
    try
        if
            false -> yes
        end
    catch
        error:if_clause -> no
    end.

tuple_size_edge(T, N) when tuple_size(T) =:= N -> ok;
tuple_size_edge(_, _) -> error.

%% Chained element access with andalso
chain_elem(T) when is_tuple(T) andalso tuple_size(T) =:= 3
                    andalso element(1, T) =:= tag
                    andalso is_atom(element(3, T)) -> ok;
chain_elem(_) -> error.

%% Guard that will throw on non-tuples (tuple_size)
guard_exception(T) when tuple_size(T) =:= 3 -> yes;
guard_exception(_) -> no.

%% Guard arithmetic: div by zero should cause guard failure
guard_arith(A, B) when (A div B) > 0 -> ok;
guard_arith(_, _) -> error.

%% andalso short-circuits: second operand exception won't happen
andalso_exc(A, T) when A andalso (tuple_size(T) > 0) -> true;
andalso_exc(_, _) -> false.

%% orelse short-circuits: second operand exception won't happen
orelse_exc(A, T) when A orelse (tuple_size(T) > 0) -> true;
orelse_exc(_, _) -> false.

%% Multiple BIF calls that may throw
multi_bif_fail(X) when tuple_size(X) =:= 3 -> a;
multi_bif_fail(X) when length(X) =:= 3 -> b;
multi_bif_fail(_) -> c.

%% Tricky: (A and B) is equivalent to (A andalso B) for booleans
tricky_bool(A, B) when A and B -> true;
tricky_bool(_, _) -> false.

%% Nested tuple element access in guards
nested_tuple(T) when element(2, element(1, T)) =:= 1,
                      element(2, element(2, T)) =:= 2 -> ok;
nested_tuple(_) -> error.

id(I) -> I.
