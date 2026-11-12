%% EXPECT: ok
-module(float_to_binary_short).
-export([test/0]).

test() ->
    <<"9.9e100">> = float_to_binary(9.9e100, [short]),
    <<"1.0e3">> = float_to_binary(1000.0, [short]),
    <<"0.0001">> = float_to_binary(0.0001, [short]),
    <<"1.0e-5">> = float_to_binary(0.00001, [short]),
    <<"-0.0">> = float_to_binary(-0.0, [short]),
    io:format("ok~n").
