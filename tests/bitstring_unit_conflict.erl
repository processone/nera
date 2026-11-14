%% EXPECT_COMPILE_ERROR: conflict in unit specification for bit field: '8' and '1'
%% /bitstring fixes the unit at 1, just like its /bits alias on OTP.
-module(bitstring_unit_conflict).
-export([test/0]).
test() -> construct(<<42>>).
construct(X) -> <<X/bitstring-unit:8>>.
