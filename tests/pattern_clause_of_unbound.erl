%% EXPECT_COMPILE_ERROR: variable 'K' is unbound
-module(pattern_clause_of_unbound).
-export([test/0]).

test() -> try id({route, #{route => ok}}) of {K, #{K := V}} -> V catch _:_ -> no end.

id(X) -> X.
