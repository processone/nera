%% EXPECT_COMPILE_ERROR: illegal float
%% A float literal beyond the float range is rejected, as OTP's scanner
%% rejects it, instead of compiling to an infinite float.
-module(float_literal_out_of_range).
-export([test/0]).
test() -> 1.0e309.
