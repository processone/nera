%% EXPECT_COMPILE_ERROR: bit type {unit,257} undefined
%% OTP rejects an out-of-range unit before checking the bitstring conflict.
-module(bitstring_unit_out_of_range).
-export([test/0]).
test() -> pattern(<<42>>).
pattern(<<X/unit:257-bitstring>>) -> X.
