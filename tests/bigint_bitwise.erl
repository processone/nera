-module(bigint_bitwise).
-export([test/0]).

%% Test: bitwise operations with bigint
test() ->
    %% Left shift producing bigint
    Big = 1 bsl 100,
    true = is_integer(Big),
    %% Right shift back to small int
    1 = Big bsr 100,
    %% Bitwise AND
    0 = Big band 0,
    Big = Big band Big,
    %% Bitwise OR
    Big = Big bor 0,
    Big = Big bor Big,
    %% Bitwise XOR
    Big = Big bxor 0,
    0 = Big bxor Big,
    %% Bitwise NOT on small int
    -1 = bnot 0,
    -2 = bnot 1,
    %% Mask bits from a bigint (explicit parentheses for precedence)
    255 = ((1 bsl 100) bor 255) band 255,
    ok.
