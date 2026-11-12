%% EXPECT_COMPILE_ERROR: a literal string in a binary pattern must not have a type or a size
%% NERA-106: OTP 29 refuses a non-UTF type on a string in a binary pattern
%% (typed_literal_string). Nera read "ab"/binary as two rest captures, so
%% this clause matched any non-empty binary.
-module(binary_string_pattern_typed).
-export([test/0]).
test() -> first(<<1, "ab">>).
first(<<X, "ab"/binary>>) -> X.
