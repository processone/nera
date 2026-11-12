-module(bitstring_concat).
-export([test/0]).

test() ->
    %% Two sub-byte segments adding to a full byte
    B1 = <<1:3>>,
    B2 = <<2:5>>,
    %% Concatenating via nested binary construction
    %% <<1:3, 2:5>> = <<(1 bsl 5) bor 2>> = <<34>>
    B3 = <<1:3, 2:5>>,
    8 = bit_size(B3),
    1 = byte_size(B3),
    true = is_binary(B3),

    %% Verify the concatenated value
    <<V:8>> = B3,
    34 = V,  %% (1 << 5) | 2 = 34

    %% Sub-byte result: 3 + 4 = 7 bits
    B4 = <<7:3, 15:4>>,
    7 = bit_size(B4),
    1 = byte_size(B4),
    false = is_binary(B4),
    true = is_bitstring(B4),

    ok.
