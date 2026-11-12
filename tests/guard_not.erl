%% Adapted from OTP compiler/test/guard_SUITE.erl (Apache 2.0)
%% Tests the 'not' operator in guards.
-module(guard_not).
-export([test/0]).

test() ->
    %% Basic not with type tests
    true = not_is_integer(hello),
    false = not_is_integer(42),
    true = not_is_atom(42),
    false = not_is_atom(hello),

    %% not with comparisons
    true = not_less(5, 3),
    false = not_less(3, 5),
    true = not_less(5, 5),

    %% not not (double negation)
    true = not_not_true(true),
    false = not_not_true(false),

    %% not combined with comma (conjunction)
    yes = not_and_comma(hello, 42),
    no = not_and_comma(42, hello),
    no = not_and_comma(hello, hello),
    no = not_and_comma(42, 42),

    %% not combined with semicolon (disjunction)
    yes = not_or_semi(hello, 42),
    no = not_or_semi(42, hello),
    yes = not_or_semi(hello, hello),
    yes = not_or_semi(42, 42),

    %% not with guard BIFs that may throw — should fail gracefully
    false = safe_not_length(not_a_list),
    true = safe_not_length([]),
    false = safe_not_length([a]),

    %% Complex not expressions
    ok = complex_not(1, 10, hello),
    fail = complex_not(10, 1, hello),
    fail = complex_not(1, 10, []),
    fail = complex_not(10, 1, []),

    %% Nested not (from OTP nested_nots)
    ok = nested_not(id(a), id(b)),
    ok = nested_not(id(a), id(a)),
    ok = nested_not(id(1), id(1)),

    io:format("ok~n"),
    ok.

not_is_integer(X) when not is_integer(X) -> true;
not_is_integer(_) -> false.

not_is_atom(X) when not is_atom(X) -> true;
not_is_atom(_) -> false.

not_less(A, B) when not (A < B) -> true;
not_less(_, _) -> false.

not_not_true(X) when not not X -> true;
not_not_true(_) -> false.

not_and_comma(A, B) when not is_integer(A), not is_atom(B) -> yes;
not_and_comma(_, _) -> no.

not_or_semi(A, B) when not is_integer(A) -> yes;
not_or_semi(A, B) when not is_atom(B) -> yes;
not_or_semi(_, _) -> no.

safe_not_length(X) when not is_list(X) -> false;
safe_not_length(X) when length(X) == 0 -> true;
safe_not_length(_) -> false.

%% not (A < B) andalso not (B < C) andalso not is_list(D)
complex_not(A, B, D) when not (A < B) andalso not is_list(D) -> fail;
complex_not(A, B, D) when not (A > B) andalso not is_list(D) -> ok;
complex_not(_, _, _) -> fail.

%% Nested not: not (((X>Y) or not(is_atom(X))) and (is_atom(Y) or (X==3.4)))
%% This should succeed when the nested condition is false.
nested_not(X, Y) when not (((X>Y) or not(is_atom(X))) and (is_atom(Y) or (X==3.4))) -> ok;
nested_not(_, _) -> ok.

id(I) -> I.
