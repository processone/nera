-module(tco_xmod_helper).
-export([bounce_back/1]).

bounce_back(0) -> ok;
bounce_back(N) -> tco_cross_module:bounce(N - 1).
