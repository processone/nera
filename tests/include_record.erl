-module(include_record).
-export([test/0]).

-include("include/test_record.hrl").

%% Test that records defined in included header files work correctly:
%% creation, field access, pattern matching, and update.

test() ->
    %% Create record from include
    P = #point{x=3, y=4},
    3 = P#point.x,
    4 = P#point.y,

    %% Pattern match on included record
    #point{x=3, y=4} = P,

    %% Record update
    P2 = P#point{x=10},
    10 = P2#point.x,
    4 = P2#point.y,

    %% Nested record from include
    R = #rect{origin=P, width=100, height=50},
    100 = R#rect.width,
    #point{x=3, y=4} = R#rect.origin,

    ok.
