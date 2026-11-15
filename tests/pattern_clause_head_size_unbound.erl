%% EXPECT_COMPILE_ERROR: variable 'N' is unbound
-module(pattern_clause_head_size_unbound).
-export([test/0]).

test() -> f(8, <<5>>).
f(N, <<V:N>>) -> V.
