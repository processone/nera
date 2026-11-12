%% Adapted from OTP compiler/test/guard_SUITE.erl (Apache 2.0)
%% Tests 'and'/'or' boolean operators in guards.
-module(guard_and_or).
-export([test/0]).

test() ->
    %% Basic and
    true = and_test(true, true),
    false = and_test(true, false),
    false = and_test(false, true),
    false = and_test(false, false),

    %% Basic or
    true = or_test(true, true),
    true = or_test(true, false),
    true = or_test(false, true),
    false = or_test(false, false),

    %% and with type tests
    yes = and_type(42, hello),
    no = and_type(hello, hello),
    no = and_type(42, 42),
    no = and_type(hello, 42),

    %% or with type tests
    yes = or_type(42, hello),
    yes = or_type(hello, hello),
    yes = or_type(42, 42),
    no = or_type(3.14, []),

    %% and with comparisons
    ok = and_range(5, 1, 10),
    out = and_range(0, 1, 10),
    out = and_range(11, 1, 10),

    %% or with comparisons
    yes = or_cmp(0, 10),
    yes = or_cmp(10, 0),
    yes = or_cmp(0, 0),
    no = or_cmp(5, 5),

    %% Complex and/or with tuple_size
    ok = relprod({set, a, b}, {set, c, d}),
    no = relprod({bag, a, b}, {set, c, d}),
    no = relprod({set, a, b}, {bag, c, d}),
    no = relprod(not_a_tuple, {set, c, d}),

    %% and/or with variables (prevent constant folding)
    true = and_var(id(true), id(true)),
    false = and_var(id(true), id(false)),
    false = and_var(id(false), id(true)),
    false = and_var(id(false), id(false)),

    true = or_var(id(true), id(true)),
    true = or_var(id(true), id(false)),
    true = or_var(id(false), id(true)),
    false = or_var(id(false), id(false)),

    %% Non-boolean operands cause guard failure (not crash)
    no = and_guard_fail(1, true),
    no = and_guard_fail(true, 1),
    no = and_guard_fail(hello, world),
    no = or_guard_fail(1, false),
    no = or_guard_fail(false, 1),
    no = or_guard_fail(1, true),
    no = or_guard_fail(hello, world),

    %% Non-boolean operands raise badarg outside guards
    ok = badarg_and(1, true),
    ok = badarg_or(1, true),
    ok = badarg_not(42),

    %% More or guards (from OTP more_or_guards)
    ok = more_or1(id({a,b,c})),
    ok = more_or1(id({a,b,c,d})),
    error = more_or1(id({a,b})),
    error = more_or1(id(atom)),

    ok = more_or2(id({a,b,c})),
    ok = more_or2(id({a,b,c,d})),
    error = more_or2(id({a,b})),
    error = more_or2(id(atom)),

    io:format("ok~n"),
    ok.

and_test(A, B) when A and B -> true;
and_test(_, _) -> false.

or_test(A, B) when A or B -> true;
or_test(_, _) -> false.

and_type(A, B) when is_integer(A) and is_atom(B) -> yes;
and_type(_, _) -> no.

or_type(A, B) when is_integer(A) or is_atom(B) -> yes;
or_type(_, _) -> no.

and_range(X, Low, High) when (X >= Low) and (X =< High) -> ok;
and_range(_, _, _) -> out.

or_cmp(A, B) when (A =< 0) or (B =< 0) -> yes;
or_cmp(_, _) -> no.

%% relprod pattern from OTP
relprod(R1, R2) when (tuple_size(R1) =:= 3) and (element(1,R1) =:= set)
                 and (tuple_size(R2) =:= 3) and (element(1,R2) =:= set) -> ok;
relprod(_, _) -> no.

and_var(A, B) when A and B -> true;
and_var(_, _) -> false.

or_var(A, B) when A or B -> true;
or_var(_, _) -> false.

%% Non-boolean operands: guard should fail, not crash
and_guard_fail(A, B) when A and B -> yes;
and_guard_fail(_, _) -> no.

%% Non-boolean operands raise badarg outside guards
badarg_and(A, B) ->
    try A and B of
        _ -> error
    catch
        error:badarg -> ok
    end.

badarg_or(A, B) ->
    try A or B of
        _ -> error
    catch
        error:badarg -> ok
    end.

badarg_not(A) ->
    try not A of
        _ -> error
    catch
        error:badarg -> ok
    end.

or_guard_fail(A, B) when A or B -> yes;
or_guard_fail(_, _) -> no.

%% More or guards: tuple_size ranges with or
more_or1(T) when is_tuple(T) and ((tuple_size(T) =:= 3) or (tuple_size(T) =:= 4)) -> ok;
more_or1(_) -> error.

more_or2(T) when is_tuple(T), (tuple_size(T) =:= 3) or (tuple_size(T) =:= 4) -> ok;
more_or2(_) -> error.

id(I) -> I.
