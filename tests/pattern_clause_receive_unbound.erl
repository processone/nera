%% EXPECT_COMPILE_ERROR: variable 'K' is unbound
-module(pattern_clause_receive_unbound).
-export([test/0]).

test() -> receive {K, #{K := V}} -> V after 0 -> ok end.
