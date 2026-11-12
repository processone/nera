%% Adapted from OTP compiler/test/guard_SUITE.erl (Apache 2.0)
%% Tests 'xor' boolean operator in guards.
-module(guard_xor).
-export([test/0]).

test() ->
    %% Exhaustive truth table with literals
    true = xor_lit(true, false),
    true = xor_lit(false, true),
    false = xor_lit(true, true),
    false = xor_lit(false, false),

    %% With variables (prevent constant folding)
    true = xor_var(id(true), id(false)),
    true = xor_var(id(false), id(true)),
    false = xor_var(id(true), id(true)),
    false = xor_var(id(false), id(false)),

    %% xor in guard with type tests
    yes = xor_type(42, hello),
    yes = xor_type(hello, 42),
    no = xor_type(42, 43),
    no = xor_type(hello, world),

    %% Non-boolean operands cause guard failure (badarg), not crash
    no = xor_nonbool(1, false),
    no = xor_nonbool(true, 1),
    no = xor_nonbool(hello, world),

    %% xor combined with other boolean ops
    true = xor_and_not(id(true), id(true)),
    false = xor_and_not(id(false), id(true)),
    true = xor_and_not(id(false), id(false)),
    false = xor_and_not(id(true), id(false)),

    %% xor in if expression
    a = xor_if(id(true), id(false)),
    a = xor_if(id(false), id(true)),
    b = xor_if(id(true), id(true)),
    b = xor_if(id(false), id(false)),

    io:format("ok~n"),
    ok.

xor_lit(A, B) when A xor B -> true;
xor_lit(_, _) -> false.

xor_var(A, B) when A xor B -> true;
xor_var(_, _) -> false.

xor_type(A, B) when is_integer(A) xor is_integer(B) -> yes;
xor_type(_, _) -> no.

xor_nonbool(A, B) when A xor B -> yes;
xor_nonbool(_, _) -> no.

%% xor combined with not: (not A) xor B
xor_and_not(A, B) when (not A) xor B -> true;
xor_and_not(_, _) -> false.

xor_if(A, B) ->
    if
        A xor B -> a;
        true -> b
    end.

id(I) -> I.
