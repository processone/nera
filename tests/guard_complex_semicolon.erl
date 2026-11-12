%% Adapted from OTP compiler/test/guard_SUITE.erl (Apache 2.0)
%% Tests complex guard disjunction (semicolon) patterns.
-module(guard_complex_semicolon).
-export([test/0]).

test() ->
    %% csemi1: tuple_size + element checks across disjunctions
    1 = csemi1({a}, 1),
    2 = csemi1({a,b}, 2),
    3 = csemi1({a,b,c}, 3),
    error = csemi1({a,b,c,d}, 4),
    error = csemi1(not_a_tuple, 1),

    %% csemi2: mixed type tests in disjunction
    int = csemi2(42),
    atom = csemi2(hello),
    other = csemi2(3.14),
    other = csemi2([]),

    %% csemi3: tuple_size disjunction with element checks
    ok = csemi3({tag, a, b}),
    ok = csemi3({tag, a, b, c}),
    error = csemi3({other, a, b}),
    error = csemi3({tag, a}),

    %% csemi4: guard BIFs across disjunction branches
    ok = csemi4(42, hello),
    ok = csemi4(hello, 42),
    error = csemi4(42, 42),
    error = csemi4(hello, hello),

    %% csemi5: complex nested disjunctions
    a = csemi5(id(1), id(0)),
    b = csemi5(id(0), id(1)),
    c = csemi5(id(0), id(0)),
    a = csemi5(id(2), id(0)),

    %% csemi6: tuple with element comparisons across branches
    ok = csemi6({a, 1}),
    ok = csemi6({b, 2}),
    ok = csemi6({c, 3}),
    error = csemi6({a, 2}),
    error = csemi6({d, 1}),
    error = csemi6(not_a_tuple),

    io:format("ok~n"),
    ok.

csemi1(T, S) when tuple_size(T) =:= 1 -> S;
csemi1(T, S) when tuple_size(T) =:= 2 -> S;
csemi1(T, S) when tuple_size(T) =:= 3 -> S;
csemi1(_, _) -> error.

csemi2(X) when is_integer(X) -> int;
csemi2(X) when is_atom(X) -> atom;
csemi2(_) -> other.

csemi3(T) when tuple_size(T) =:= 3, element(1, T) =:= tag ->
    ok;
csemi3(T) when tuple_size(T) =:= 4, element(1, T) =:= tag ->
    ok;
csemi3(_) ->
    error.

csemi4(A, B) when is_integer(A), is_atom(B) -> ok;
csemi4(A, B) when is_atom(A), is_integer(B) -> ok;
csemi4(_, _) -> error.

csemi5(A, B) when A > 0 -> a;
csemi5(A, B) when B > 0 -> b;
csemi5(_, _) -> c.

csemi6(T) when element(1, T) =:= a, element(2, T) =:= 1 -> ok;
csemi6(T) when element(1, T) =:= b, element(2, T) =:= 2 -> ok;
csemi6(T) when element(1, T) =:= c, element(2, T) =:= 3 -> ok;
csemi6(_) -> error.

id(I) -> I.
