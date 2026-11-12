%% Adapted from OTP compiler/test/guard_SUITE.erl (Apache 2.0)
%% Tests basic guard expressions and type tests.
-module(guard_basic).
-export([test/0]).

test() ->
    %% Type test guards
    integer = type_of(42),
    float = type_of(3.14),
    atom = type_of(hello),
    tuple = type_of({1, 2}),
    list = type_of([1, 2]),
    pid = type_of(self()),
    binary = type_of(<<1, 2, 3>>),

    %% Guard with comparison
    positive = sign(5),
    negative = sign(-3),
    zero = sign(0),

    %% Guard with multiple tests
    ok = check_range(5, 1, 10),
    out = check_range(15, 1, 10),
    ok = check_range(1, 1, 10),
    ok = check_range(10, 1, 10),

    %% Guard with boolean operators
    true = both_positive(1, 2),
    false = both_positive(-1, 2),
    false = both_positive(1, -2),
    false = both_positive(-1, -2),

    %% Guard with is_number
    number = num_or_atom(42),
    number = num_or_atom(3.14),
    atom = num_or_atom(hello),

    %% Guard arithmetic
    ok = check_even(4),
    odd = check_even(5),
    ok = check_even(0),
    odd = check_even(1),

    %% Guard sequences (semicolon = disjunction)
    true = is_int_or_atom(42),
    true = is_int_or_atom(hello),
    false = is_int_or_atom(3.14),
    false = is_int_or_atom([1, 2]),

    %% Guard BIF calls
    2 = guard_abs(-2),
    3 = guard_abs(3),
    1 = guard_length([a]),
    3 = guard_length([a, b, c]),

    %% Guard with tuple_size
    ok = check_tuple_size({a, b, c}, 3),
    mismatch = check_tuple_size({a, b}, 3),

    io:format("ok~n"),
    ok.

type_of(X) when is_integer(X) -> integer;
type_of(X) when is_float(X) -> float;
type_of(X) when is_atom(X) -> atom;
type_of(X) when is_tuple(X) -> tuple;
type_of(X) when is_list(X) -> list;
type_of(X) when is_pid(X) -> pid;
type_of(X) when is_binary(X) -> binary.

sign(X) when X > 0 -> positive;
sign(X) when X < 0 -> negative;
sign(0) -> zero.

check_range(X, Low, High) when X >= Low, X =< High -> ok;
check_range(_, _, _) -> out.

both_positive(X, Y) when X > 0, Y > 0 -> true;
both_positive(_, _) -> false.

num_or_atom(X) when is_number(X) -> number;
num_or_atom(X) when is_atom(X) -> atom.

check_even(X) when X rem 2 == 0 -> ok;
check_even(_) -> odd.

is_int_or_atom(X) when is_integer(X) -> true;
is_int_or_atom(X) when is_atom(X) -> true;
is_int_or_atom(_) -> false.

guard_abs(X) when X < 0 -> -X;
guard_abs(X) -> X.

guard_length(L) when length(L) == 1 -> 1;
guard_length(L) when length(L) == 3 -> 3;
guard_length(L) -> length(L).

check_tuple_size(T, N) when tuple_size(T) == N -> ok;
check_tuple_size(_, _) -> mismatch.

id(I) -> I.
