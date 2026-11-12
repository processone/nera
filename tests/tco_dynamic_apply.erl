%% Tests dynamic apply in tail position via trampoline.
%% M:F(Args) where M and F are variables — cannot be resolved at compile time.
%% Without the trampoline, 100k iterations would overflow the stack.
-module(tco_dynamic_apply).
-export([test/0, ping/2, dynamic_ping/1]).

test() ->
    ok = dynamic_ping(100000),
    ok.

dynamic_ping(0) -> ok;
dynamic_ping(N) ->
    M = tco_dynamic_apply,
    F = ping,
    M:F(N - 1, dynamic).

ping(0, _) -> ok;
ping(N, dynamic) ->
    M = tco_dynamic_apply,
    M:dynamic_ping(N - 1).
