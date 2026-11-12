%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (geometry1.erl)
%% Tests: tuple pattern matching, arithmetic
-module(jaerlang2_geometry).
-export([test/0, area/1]).

test() ->
    12  = area({rectangle, 3, 4}),
    144 = area({square, 12}),
    ok.

area({rectangle, Width, Height}) -> Width * Height;
area({square, Side})             -> Side * Side.
