%% EXPECT_COMPILE_ERROR: variable 'K' is unbound
%% NERA-80 / NERA-54 I-1: a sibling fun argument cannot supply a map key.
-module(pattern_input_fun_key_unbound).
-export([test/0]).
test() ->
    F = fun(K, #{K := V}) -> V end,
    F(route, #{route => 5}).
