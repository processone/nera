%% EXPECT_COMPILE_ERROR: variable 'K' is unbound
-module(pattern_clause_else_unbound).
-export([test/0]).

test() -> maybe ok ?= id(no) else {K, #{K := V}} -> V end.

id(X) -> X.
