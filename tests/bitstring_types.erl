-module(bitstring_types).
-export([test/0]).

test() ->
    %% Sub-byte bitstring is NOT a binary
    B = <<5:3>>,
    false = is_binary(B),
    true = is_bitstring(B),

    %% Byte-aligned binary IS a binary and a bitstring
    B2 = <<1, 2, 3>>,
    true = is_binary(B2),
    true = is_bitstring(B2),

    %% Empty binary is both
    true = is_binary(<<>>),
    true = is_bitstring(<<>>),

    %% Non-binary types
    false = is_binary(42),
    false = is_bitstring(hello),

    ok.
