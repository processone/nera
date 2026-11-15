%% EXPECT_COMPILE_ERROR: variable 'K' is unbound
-module(pattern_clause_case_unbound).
-export([test/0]).

test() -> case id({route, #{route => ok}}) of {K, #{K := V}} -> V end.

id(X) -> X.
