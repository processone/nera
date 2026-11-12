%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (types1.erl)
%% Tests: boolean pattern matching, multi-clause functions
-module(jaerlang2_bool).
-export([test/0]).

test() ->
    %% myand
    true  = myand(true, true),
    false = myand(false, true),
    false = myand(true, false),
    false = myand(false, false),

    %% myor
    true  = myor(true, true),
    true  = myor(false, true),
    true  = myor(true, false),
    false = myor(false, false),

    %% mynot
    false = mynot(true),
    true  = mynot(false),

    %% myxor
    false = myxor(true, true),
    true  = myxor(false, true),
    true  = myxor(true, false),
    false = myxor(false, false),

    ok.

myand(true, true)   -> true;
myand(false, _)     -> false;
myand(_, false)     -> false.

myor(false, false) -> false;
myor(_, _)         -> true.

mynot(true)  -> false;
mynot(false) -> true.

myxor(X, X) -> false;
myxor(_, _) -> true.
