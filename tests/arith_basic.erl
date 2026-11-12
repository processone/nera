%% Adapted from OTP compiler test suites (Apache 2.0)
%% Tests basic arithmetic operations.
-module(arith_basic).
-export([test/0]).

test() ->
    %% Basic integer arithmetic
    3 = id(1) + id(2),
    1 = id(3) - id(2),
    6 = id(2) * id(3),
    2 = id(6) div id(3),
    1 = id(7) rem id(3),

    %% Unary negation
    -5 = -id(5),
    5 = -id(-5),
    0 = -id(0),

    %% Nested expressions
    14 = id(2) + id(3) * id(4),
    20 = (id(2) + id(3)) * id(4),
    2 = id(10) div id(3) - id(1),

    %% Division and remainder edge cases
    0 = id(0) div id(5),
    0 = id(0) rem id(5),
    -2 = id(-7) div id(3),
    -1 = id(-7) rem id(3),

    %% NOTE: Float arithmetic (id(1.0) + id(2.0)) is currently broken
    %% at runtime -- floats via id() return 0. See arith_float.erl for
    %% the skipped float tests.

    %% Bitwise operations
    3 = id(1) bor id(2),
    0 = id(1) band id(2),
    3 = id(1) bxor id(2),
    -1 = bnot id(0),
    4 = id(1) bsl id(2),
    1 = id(4) bsr id(2),

    io:format("ok~n"),
    ok.

id(I) -> I.
