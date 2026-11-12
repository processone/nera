%% Tests cross-module tail call optimization via return_call.
%% Without true TCO, this would overflow at ~10000 frames.
%% MODULES: _tco_xmod_helper.erl
-module(tco_cross_module).
-export([test/0, bounce/1]).

test() ->
    ok = bounce(100000),
    ok.

bounce(0) -> ok;
bounce(N) -> tco_xmod_helper:bounce_back(N - 1).
