-module(base_n_integer).
-export([test/0]).

%% Tests for base-N integer literals (Base#Digits)

test() ->
    %% Hexadecimal (base 16)
    255 = 16#FF,
    255 = 16#ff,
    10 = 16#A,
    0 = 16#0,
    256 = 16#100,

    %% Binary (base 2)
    10 = 2#1010,
    255 = 2#11111111,
    0 = 2#0,
    1 = 2#1,

    %% Octal (base 8)
    63 = 8#77,
    8 = 8#10,
    0 = 8#0,

    %% Base 36
    35 = 36#Z,
    10 = 36#A,

    %% Arithmetic with base-N integers
    true = 16#FF =:= 255,
    true = (2#1010 + 16#A) =:= 20,

    %% In pattern matching
    255 = id(16#FF),
    10 = id(2#1010),

    io:format("ok~n"),
    ok.

id(X) -> X.
