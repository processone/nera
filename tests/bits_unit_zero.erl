%% EXPECT_COMPILE_ERROR: bit type {unit,0} undefined
%% Zero is not a valid unit and must not become the alias's default unit:1.
-module(bits_unit_zero).
-export([test/0]).
test() -> construct(<<42>>).
construct(X) -> <<X/bits-unit:0>>.
