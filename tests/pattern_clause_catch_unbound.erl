%% EXPECT_COMPILE_ERROR: variable 'K' is unbound
-module(pattern_clause_catch_unbound).
-export([test/0]).

test() -> try id(ok) catch K:#{K := V} -> V end.

id(X) -> X.
