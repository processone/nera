-module(binary_bits).
-export([test/0]).

%% Test bit-level binary construction and pattern matching.
%% Sub-byte segments, unit specifiers, mixed byte/sub-byte.

test() ->
    %% Sub-byte construction: 3-bit value
    Bin3 = <<5:3>>,
    1 = byte_size(Bin3),

    %% Nibble extraction (4+4 = 8 bits)
    <<X:4, Y:4>> = <<16#AB>>,
    10 = X,
    11 = Y,

    %% IP header parsing (the canonical Erlang binary example)
    Pkt = <<16#45, 0, 0, 40>>,
    <<Version:4, IHL:4, _DSCP:6, _ECN:2, TotalLen:16>> = Pkt,
    4 = Version,
    5 = IHL,
    40 = TotalLen,

    %% Mixed byte and sub-byte
    <<A:3, B:5, C:8>> = <<16#FF, 16#AA>>,
    7 = A,    % top 3 bits of 0xFF = 111 = 7
    31 = B,   % next 5 bits of 0xFF = 11111 = 31
    170 = C,  % 0xAA = 170

    %% Construction with sub-byte segments
    Bin2 = <<7:3, 5:5>>,
    1 = byte_size(Bin2),
    <<7:3, 5:5>> = Bin2,

    %% 16-bit integer
    <<256:16>> = <<1, 0>>,

    %% Rest after sub-byte (at byte boundary)
    <<_:4, _:4, Rest/binary>> = <<16#FF, 1, 2, 3>>,
    <<1, 2, 3>> = Rest,

    %% bit_size/1
    16 = bit_size(<<0, 0>>),
    8 = bit_size(<<42>>),

    %% byte_size/1 still works
    2 = byte_size(<<0, 0>>),
    1 = byte_size(<<42>>),

    %% Multi-byte construction with sub-byte segments
    Bin4 = <<3:2, 1:1, 0:5, 16#FF:8>>,
    2 = byte_size(Bin4),
    <<3:2, 1:1, 0:5, 255:8>> = Bin4,

    ok.
