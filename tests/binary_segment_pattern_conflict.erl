%% EXPECT_COMPILE_ERROR: conflict in type specification for bit field: 'binary' and 'utf8'
%% NERA-112: the shared type-list merger checks patterns in either order too.
-module(binary_segment_pattern_conflict).
-export([test/0]).
test() -> pattern(<<1>>).
pattern(<<X/utf8-signed-binary>>) -> X.
