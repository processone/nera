%% Adapted from OTP compiler/test/guard_SUITE.erl (Apache 2.0)
%% Tests 'if' expressions with complex guards.
-module(guard_if_expr).
-export([test/0]).

test() ->
    %% if with not
    a = if_not(id(false)),
    b = if_not(id(true)),

    %% if with and
    a = if_and(id(true), id(true)),
    b = if_and(id(true), id(false)),
    b = if_and(id(false), id(true)),
    b = if_and(id(false), id(false)),

    %% if with or
    a = if_or(id(true), id(true)),
    a = if_or(id(true), id(false)),
    a = if_or(id(false), id(true)),
    b = if_or(id(false), id(false)),

    %% if with andalso/orelse
    a = if_andalso(id(true), id(true)),
    b = if_andalso(id(true), id(false)),
    b = if_andalso(id(false), id(true)),
    b = if_andalso(id(false), id(false)),

    a = if_orelse(id(true), id(true)),
    a = if_orelse(id(true), id(false)),
    a = if_orelse(id(false), id(true)),
    b = if_orelse(id(false), id(false)),

    %% if with guard BIF calls
    int = if_type(id(42)),
    atom = if_type(id(hello)),
    other = if_type(id(3.14)),

    %% if_clause exception when no guard matches
    ok = check_if_clause(id(42)),

    %% if with comparisons
    less = if_cmp(id(1), id(2)),
    greater = if_cmp(id(3), id(2)),
    equal = if_cmp(id(2), id(2)),

    %% Complex: if with multiple boolean ops
    a = if_complex(id(true), id(false), id(true)),
    b = if_complex(id(false), id(false), id(false)),

    %% check/2 pattern from OTP (using literals since id/1 is not a guard BIF)
    ok = check(fun() -> if true -> ok end end, ok),

    io:format("ok~n"),
    ok.

if_not(X) ->
    if
        not X -> a;
        true -> b
    end.

if_and(X, Y) ->
    if
        X and Y -> a;
        true -> b
    end.

if_or(X, Y) ->
    if
        X or Y -> a;
        true -> b
    end.

if_andalso(X, Y) ->
    if
        X andalso Y -> a;
        true -> b
    end.

if_orelse(X, Y) ->
    if
        X orelse Y -> a;
        true -> b
    end.

if_type(X) ->
    if
        is_integer(X) -> int;
        is_atom(X) -> atom;
        true -> other
    end.

check_if_clause(X) ->
    try
        if
            X =:= hello -> should_not_match
        end
    catch
        error:if_clause -> ok
    end.

if_cmp(A, B) ->
    if
        A < B -> less;
        A > B -> greater;
        true -> equal
    end.

if_complex(A, B, C) ->
    if
        (A or B) andalso C -> a;
        true -> b
    end.

check(F, Expected) ->
    case catch F() of
        Expected -> ok;
        Other -> error({check_failed, Expected, Other})
    end.

id(I) -> I.
