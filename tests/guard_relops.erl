%% Adapted from OTP compiler/test/guard_SUITE.erl (Apache 2.0)
%% Tests exhaustive relational operator behavior in guards.
-module(guard_relops).
-export([test/0]).

test() ->
    %% == vs =:= with int and float
    true = eq_test(1, 1.0),
    false = exact_eq_test(1, 1.0),
    true = exact_eq_test(1, 1),
    true = exact_eq_test(1.0, 1.0),

    %% /= vs =/=
    false = neq_test(1, 1.0),
    true = exact_neq_test(1, 1.0),
    false = exact_neq_test(1, 1),

    %% < > =< >= with integers
    true = lt_test(1, 2),
    false = lt_test(2, 1),
    false = lt_test(1, 1),

    true = gt_test(2, 1),
    false = gt_test(1, 2),
    false = gt_test(1, 1),

    true = lteq_test(1, 2),
    true = lteq_test(1, 1),
    false = lteq_test(2, 1),

    true = gteq_test(2, 1),
    true = gteq_test(1, 1),
    false = gteq_test(1, 2),

    %% Mixed int/float comparisons
    true = lt_test(1, 1.5),
    false = lt_test(2, 1.5),
    true = gt_test(2, 1.5),
    false = gt_test(1, 1.5),

    %% Atom ordering (atoms are compared by name)
    true = lt_test(a, b),
    false = lt_test(b, a),
    true = eq_test(hello, hello),
    false = eq_test(hello, world),

    %% Guards using each relational op
    pos = sign_guard(5),
    neg = sign_guard(-3),
    zero = sign_guard(0),

    %% Relational ops in complex guards
    in = range_guard(5, 1, 10),
    out = range_guard(0, 1, 10),
    out = range_guard(11, 1, 10),

    %% All 8 ops with variables (prevent constant folding)
    true = all_ops(id(1), id(2)),

    io:format("ok~n"),
    ok.

eq_test(A, B) when A == B -> true;
eq_test(_, _) -> false.

exact_eq_test(A, B) when A =:= B -> true;
exact_eq_test(_, _) -> false.

neq_test(A, B) when A /= B -> true;
neq_test(_, _) -> false.

exact_neq_test(A, B) when A =/= B -> true;
exact_neq_test(_, _) -> false.

lt_test(A, B) when A < B -> true;
lt_test(_, _) -> false.

gt_test(A, B) when A > B -> true;
gt_test(_, _) -> false.

lteq_test(A, B) when A =< B -> true;
lteq_test(_, _) -> false.

gteq_test(A, B) when A >= B -> true;
gteq_test(_, _) -> false.

sign_guard(X) when X > 0 -> pos;
sign_guard(X) when X < 0 -> neg;
sign_guard(X) when X == 0 -> zero.

range_guard(X, Lo, Hi) when X >= Lo, X =< Hi -> in;
range_guard(_, _, _) -> out.

%% Test all 8 relational ops with variable operands
all_ops(A, B) when A < B, A =< B, B > A, B >= A,
                   A /= B, A =/= B, A == A, A =:= A -> true;
all_ops(_, _) -> false.

id(I) -> I.
