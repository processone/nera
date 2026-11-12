%% EXPECT_COMPILE_ERROR: unresolved function `nonexistent_module:do_stuff/1`
%% Tests that calling a function in a module that doesn't exist
%% is caught at compile time rather than silently compiling.
-module(unresolved_call).
-export([test/0]).

test() ->
    nonexistent_module:do_stuff(42).
