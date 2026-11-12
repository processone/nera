%% EXPECT_COMPILE_ERROR: binary fields without size are not allowed in patterns of bit string generators
%% NERA-73 F3: reject this pattern instead of generating a non-consuming loop.
-module(binary_generator_unsized).
-export([test/0]).
test() -> [X || <<X/binary>> <= id(<<>>)].
id(X) -> X.
