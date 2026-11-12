%% EXPECT_COMPILE_ERROR: a literal string in a binary pattern must not have a type or a size
%% NERA-106: OTP 29 refuses a sized string in a binary pattern
%% (typed_literal_string). Nera dropped the size and matched <<"ab", ...>>.
-module(binary_string_pattern_sized).
-export([test/0]).
test() -> rest(<<0, $a, 0, $b, "rest">>).
rest(<<"ab":16, R/binary>>) -> R.
