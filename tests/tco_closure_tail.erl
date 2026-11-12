%% Tests closure call in tail position via trampoline.
%% Fun(N-1) where Fun is a closure — dispatched dynamically.
%% Without the trampoline, 100k iterations would overflow the stack.
-module(tco_closure_tail).
-export([test/0]).

test() ->
    F = fun loop_fun/1,
    ok = F(100000),
    ok.

loop_fun(0) -> ok;
loop_fun(N) ->
    F = fun loop_fun/1,
    F(N - 1).
