-module(bitstring_pattern).
-export([test/0]).

test() ->
    %% Pattern match on a 3-bit bitstring
    <<X:3>> = <<5:3>>,
    5 = X,

    %% Pattern match with multiple sub-byte segments
    <<A:4, B:4>> = <<10:4, 7:4>>,
    10 = A,
    7 = B,

    %% Pattern match extracting from larger bitstring
    <<C:5, D:3>> = <<25:5, 6:3>>,
    25 = C,
    6 = D,

    ok.
