%% EXPECT_COMPILE_ERROR: variable 'N' is unbound
%% NERA-80: a later binary-generator segment cannot supply an earlier size.
-module(pattern_input_binary_unbound).
-export([test/0]).
test() -> << <<V>> || <<V:N, N>> <= <<5, 8>> >>.
