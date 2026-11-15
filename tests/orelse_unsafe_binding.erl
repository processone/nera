%% EXPECT_COMPILE_ERROR: variable 'Y' unsafe in 'orelse'
-module(orelse_unsafe_binding).
-export([test/0]).

test() ->
    _ = id(false) orelse (Y = true),
    Y.

id(Value) -> Value.
