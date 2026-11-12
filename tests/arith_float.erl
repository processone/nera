%% Adapted from OTP compiler test suites (Apache 2.0)
%% Tests float arithmetic operations.
-module(arith_float).
-export([test/0]).

test() ->
    %% Float arithmetic
    3.0 = id(1.0) + id(2.0),
    0.5 = id(1.0) / id(2.0),
    6.0 = id(2.0) * id(3.0),

    %% Mixed int/float
    3.0 = id(1) + id(2.0),
    3.0 = id(1.0) + id(2),

    %% Float comparison
    true = id(1.0) < id(2.0),
    true = id(1.0) == id(1.0),
    false = id(1.0) > id(2.0),

    io:format("ok~n"),
    ok.

id(I) -> I.
