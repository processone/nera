-module(tco_mutual).
-export([test/0]).

%% Test mutual tail-call optimization.
%% Without true TCO (return_call), this would overflow the stack
%% at ~4000 frames. With return_call, it completes successfully.

test() ->
    ok = ping(100000),
    ok.

ping(0) -> ok;
ping(N) -> pong(N - 1).

pong(0) -> ok;
pong(N) -> ping(N - 1).
