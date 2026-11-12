-module(binary_utf).
-export([test/0]).

test() ->
    test_utf8_construct(),
    test_utf8_match(),
    test_utf16_construct(),
    test_utf16_match(),
    test_utf32_construct(),
    test_utf32_match(),
    test_utf_sizes(),
    test_utf_multi_segment(),
    io:format("ok~n"),
    ok.

test_utf8_construct() ->
    %% ASCII: 1 byte
    <<97>> = <<$a/utf8>>,
    %% 2-byte: Latin e-acute (U+00E9)
    <<195, 169>> = <<16#E9/utf8>>,
    %% 3-byte: Euro sign (U+20AC)
    <<226, 130, 172>> = <<16#20AC/utf8>>,
    %% 4-byte: Grinning face emoji (U+1F600)
    <<240, 159, 152, 128>> = <<16#1F600/utf8>>,
    ok.

test_utf8_match() ->
    %% Match 2-byte UTF-8 sequence with rest
    <<C1/utf8, R1/binary>> = <<195, 169, 120>>,
    16#E9 = C1,
    <<120>> = R1,
    %% Match 4-byte UTF-8 sequence (emoji)
    <<C2/utf8>> = <<240, 159, 152, 128>>,
    16#1F600 = C2,
    %% ASCII match
    <<C3/utf8, R3/binary>> = <<"hello">>,
    $h = C3,
    <<"ello">> = R3,
    ok.

test_utf16_construct() ->
    %% BMP big-endian (default)
    <<0, 97>> = <<$a/utf16>>,
    %% BMP little-endian
    <<97, 0>> = <<$a/utf16-little>>,
    %% Supplementary plane: surrogate pair (U+1F600)
    <<216, 61, 222, 0>> = <<16#1F600/utf16>>,
    ok.

test_utf16_match() ->
    %% BMP big-endian match
    <<C1/utf16, R1/binary>> = <<0, 97, 1, 2>>,
    97 = C1,
    <<1, 2>> = R1,
    %% Surrogate pair match
    <<C2/utf16>> = <<216, 61, 222, 0>>,
    16#1F600 = C2,
    ok.

test_utf32_construct() ->
    %% Big-endian (default)
    <<0, 0, 0, 97>> = <<$a/utf32>>,
    %% Little-endian
    <<97, 0, 0, 0>> = <<$a/utf32-little>>,
    %% Non-ASCII
    <<0, 0, 0, 233>> = <<16#E9/utf32>>,
    ok.

test_utf32_match() ->
    %% Big-endian match
    <<C1/utf32>> = <<0, 0, 0, 97>>,
    97 = C1,
    %% Little-endian match
    <<C2/utf32-little>> = <<97, 0, 0, 0>>,
    97 = C2,
    ok.

test_utf_sizes() ->
    1 = byte_size(<<$a/utf8>>),
    2 = byte_size(<<16#E9/utf8>>),
    3 = byte_size(<<16#20AC/utf8>>),
    4 = byte_size(<<16#1F600/utf8>>),
    2 = byte_size(<<$a/utf16>>),
    4 = byte_size(<<16#1F600/utf16>>),
    4 = byte_size(<<$a/utf32>>),
    ok.

test_utf_multi_segment() ->
    %% Multiple UTF-8 segments = ASCII string
    Bin = <<$h/utf8, $e/utf8, $l/utf8, $l/utf8, $o/utf8>>,
    <<"hello">> = Bin,
    %% Mixed UTF and integer segments
    Mixed = <<$a/utf8, 0, $b/utf8>>,
    <<$a, 0, $b>> = Mixed,
    ok.

id(I) -> I.
