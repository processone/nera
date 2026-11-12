%% EXPECT_COMPILE_ERROR: conflict in type specification for bit field: 'utf8' and 'binary'
%% NERA-112: OTP refuses conflicting types rather than keeping the last one.
-module(binary_segment_type_conflict).
-export([test/0]).
test() -> construct(1).
construct(X) -> <<X/binary-utf8>>.
