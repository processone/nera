%% EXPECT_COMPILE_ERROR: variable 'K' is unbound
-module(pattern_clause_match_unbound).
-export([test/0]).

test() -> {K, #{K := V}} = id({route, #{route => ok}}), V.

id(X) -> X.
