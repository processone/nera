%% EXPECT_COMPILE_ERROR: variable 'K' is unbound
-module(pattern_clause_head_key_unbound).
-export([test/0]).

test() -> f(route, #{route => ok}).
f(K, #{K := V}) -> V.
