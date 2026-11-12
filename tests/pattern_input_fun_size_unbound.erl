%% EXPECT_COMPILE_ERROR: variable 'N' is unbound
%% NERA-80 / NERA-54 I-1: a sibling fun argument cannot supply a binary size.
-module(pattern_input_fun_size_unbound).
-export([test/0]).
test() ->
    F = fun(N, <<V:N>>) -> V end,
    F(8, <<5>>).
