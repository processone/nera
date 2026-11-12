%% coding: utf-8
%% NERA-112: OTP 29 accepts nonconflicting flags, repeats and binary aliases.
%% Run this same source on native Nera and OTP 29; ignored flags must not
%% change the encoded bytes or matching behavior.
%% EXPECT: binary_segment_specifiers_ok
-module(binary_segment_specifiers).
-export([test/0]).

test() ->
    lists:foreach(fun(C) ->
        U8 = <<C/utf8>>,
        U16 = <<C/utf16-little>>,
        U32 = <<C/utf32-big>>,
        U8 = <<C/utf8-signed>>,
        U8 = <<C/signed-utf8>>,
        U8 = <<C/utf8-unsigned-little>>,
        U16 = <<C/utf16-signed-little>>,
        U16 = <<C/signed-little-utf16>>,
        U32 = <<C/utf32-signed-big>>,
        U32 = <<C/big-signed-utf32>>,
        C = utf8_head(U8),
        C = utf16_head(U16),
        C = utf32_head(U32),
        C = case U8 of <<V/signed-utf8>> -> V end,
        <<C/utf16-little-signed>> = U16,
        F = fun(<<V/signed-utf32-big>>) -> V end,
        C = F(U32),
        [C] = [V || <<V/utf8-signed>> <= U8],
        U16 = << <<V/utf16-signed-little>> || V <- [C] >>
    end, [0, 65, 233, 955, 16#1f914, 16#10ffff]),
    <<1>> = <<1/utf8-signed>>,
    <<"λ"/utf8>> = <<"λ"/utf8-signed>>,
    yes = string_utf8(<<"λ"/utf8>>),
    <<1>> = <<1:1/integer-integer-unsigned-unsigned-big-big-unit:8-unit:8>>,
    <<1:1>> = <<(id(<<1:1>>))/binary-bits>>,
    <<1:1>> = <<(id(<<1:1>>))/bits-binary>>,
    <<1>> = <<(id(<<1>>))/binary-bytes-unit:8>>,
    <<1>> = <<(id(<<1>>))/bytes-binary>>,
    <<1:1>> = <<(id(<<1:1>>))/bitstring-bits-unit:1>>,
    <<1:1>> = <<(id(<<1:1>>))/unit:1-binary>>,
    <<1>> = <<(id(<<1>>))/binary-signed-little>>,
    <<1>> = binary_flags(id(<<1>>)),
    <<1:1>> = binary_aliases(id(<<1:1>>)),
    <<1.5/float>> = <<(id(1.5))/float-signed>>,
    1.5 = float_flags(id(<<1.5/float>>)),
    -1 = signed_integer(id(<<255>>)),
    io:format("binary_segment_specifiers_ok~n"),
    ok.

utf8_head(<<C/utf8-signed>>) -> C.
utf16_head(<<C/signed-utf16-little>>) -> C.
utf32_head(<<C/utf32-big-signed>>) -> C.
string_utf8(<<"λ"/utf8-signed>>) -> yes.
binary_flags(<<B/binary-signed-little>>) -> B.
binary_aliases(<<B/binary-bits>>) -> B.
float_flags(<<F/float-signed>>) -> F.
signed_integer(<<I/integer-signed>>) -> I.
id(X) -> X.
