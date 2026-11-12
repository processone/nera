%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (fac.erl)
%% Tests: recursion, integer pattern matching
-module(jaerlang2_fac).
-export([test/0, fac/1]).

test() ->
    1       = fac(0),
    1       = fac(1),
    6       = fac(3),
    120     = fac(5),
    3628800 = fac(10),
    ok.

fac(0) -> 1;
fac(N) -> N * fac(N-1).
