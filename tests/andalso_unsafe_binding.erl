%% EXPECT_COMPILE_ERROR: variable 'Y' unsafe in 'andalso'
-module(andalso_unsafe_binding).
-export([test/0]).

test() ->
    _ = id(true) andalso (Y = true),
    Y.

id(Value) -> Value.
