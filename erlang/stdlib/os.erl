-module(os).
-export([timestamp/0]).

%% Wall-clock microseconds split into the traditional timestamp representation.
timestamp() ->
    Microseconds = erlang:system_time(microsecond),
    {Microseconds div 1000000000000,
     (Microseconds div 1000000) rem 1000000,
     Microseconds rem 1000000}.
