%% EXPECT_COMPILE_ERROR: variable 'K' is unbound
%% NERA-80: list generator inputs precede every binding in its pattern.
-module(pattern_input_list_unbound).
-export([test/0]).
test() -> [V || {K, #{K := V}} <- [{route, #{route => 5}}]].
