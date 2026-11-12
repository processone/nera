%% Adapted from OTP compiler/test/bs_construct_SUITE.erl (Apache 2.0)
%% Tests binary construction.
-module(binary_construct).
-export([test/0]).

test() ->
    %% Simple binary construction
    <<1, 2, 3>> = id(<<1, 2, 3>>),
    <<>> = id(<<>>),

    %% Size specification
    <<1:16>> = id(<<0, 1>>),
    <<256:16>> = id(<<1, 0>>),

    %% Variables in binary construction
    X = 42,
    <<42>> = <<X>>,
    <<42:16>> = <<X:16>>,

    %% Multiple segments
    A = 1,
    B = 2,
    C = 3,
    <<1, 2, 3>> = <<A, B, C>>,

    %% Little endian
    <<1, 0>> = <<1:16/little>>,
    <<0, 1>> = <<1:16/big>>,

    %% byte_size
    3 = byte_size(<<1, 2, 3>>),
    0 = byte_size(<<>>),
    2 = byte_size(<<1:16>>),

    %% Binary concatenation via construction
    B1 = <<1, 2>>,
    B2 = <<3, 4>>,
    <<1, 2, 3, 4>> = <<B1/binary, B2/binary>>,

    io:format("ok~n"),
    ok.

id(I) -> I.
