-module(bitstring_basic).
-export([test/0]).

test() ->
    %% A 3-bit bitstring
    B = <<5:3>>,
    3 = bit_size(B),
    1 = byte_size(B),

    %% A full byte
    B2 = <<255:8>>,
    8 = bit_size(B2),
    1 = byte_size(B2),

    %% Sub-byte with multiple segments
    B3 = <<1:4, 2:4>>,
    8 = bit_size(B3),
    1 = byte_size(B3),

    %% 13 bits = 2 bytes
    B4 = <<100:13>>,
    13 = bit_size(B4),
    2 = byte_size(B4),

    %% Empty binary
    B5 = <<>>,
    0 = bit_size(B5),
    0 = byte_size(B5),

    ok.
