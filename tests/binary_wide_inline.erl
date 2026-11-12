-module(binary_wide_inline).
-export([test/0]).

%% Test inline binary integer reads at 8/16/32/64-bit widths,
%% both big-endian and little-endian, unsigned and signed.

test() ->
    ok = test_8bit(),
    ok = test_16bit(),
    ok = test_32bit(),
    ok = test_64bit(),
    ok = test_signed(),
    ok.

test_8bit() ->
    <<V:8>> = id(<<42>>),
    42 = V,
    ok.

test_16bit() ->
    %% Big-endian (default)
    <<V:16>> = id(<<1, 2>>),
    258 = V,   %% 1*256 + 2
    %% Little-endian
    <<VL:16/little>> = id(<<1, 2>>),
    513 = VL,  %% 2*256 + 1
    ok.

test_32bit() ->
    %% Big-endian
    <<V:32>> = id(<<0, 0, 1, 0>>),
    256 = V,
    %% Little-endian
    <<VL:32/little>> = id(<<0, 1, 0, 0>>),
    256 = VL,
    ok.

test_64bit() ->
    %% Big-endian
    <<V:64>> = id(<<0, 0, 0, 0, 0, 0, 1, 0>>),
    256 = V,
    %% Little-endian
    <<VL:64/little>> = id(<<0, 1, 0, 0, 0, 0, 0, 0>>),
    256 = VL,
    ok.

test_signed() ->
    %% 8-bit signed
    <<S8:8/signed>> = id(<<255>>),
    -1 = S8,
    %% 16-bit signed big-endian
    <<S16:16/signed>> = id(<<255, 254>>),
    -2 = S16,
    %% 16-bit signed little-endian
    <<S16L:16/signed-little>> = id(<<254, 255>>),
    -2 = S16L,
    %% 32-bit signed
    <<S32:32/signed>> = id(<<255, 255, 255, 254>>),
    -2 = S32,
    ok.

id(X) -> X.
