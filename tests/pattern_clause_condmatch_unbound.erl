%% EXPECT_COMPILE_ERROR: variable 'K' is unbound
-module(pattern_clause_condmatch_unbound).
-export([test/0]).

test() -> maybe {K, #{K := V}} ?= id({route, #{route => ok}}), V end.

id(X) -> X.
