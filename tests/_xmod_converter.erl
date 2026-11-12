-module(xmod_converter).
-export([celsius_to_fahrenheit/1, fahrenheit_to_celsius/1, double_convert/1]).

%% Helper module for bidirectional cross-module call tests.

celsius_to_fahrenheit(C) -> C * 9 / 5 + 32.

fahrenheit_to_celsius(F) -> (F - 32) * 5 / 9.

%% Calls back into xmod_bidir to test bidirectional linking.
double_convert(C) ->
    F = celsius_to_fahrenheit(C),
    xmod_bidir:round_trip_check(C, F).
