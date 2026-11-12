-module(xmod_bidir).
-export([test/0, round_trip_check/2]).
%% MODULES: _xmod_converter.erl

%% Test bidirectional cross-module calls.
%% xmod_bidir calls xmod_converter, and xmod_converter calls back into xmod_bidir.

round_trip_check(_C, _F) ->
    ok.

test() ->
    %% Basic cross-module calls
    212.0 = xmod_converter:celsius_to_fahrenheit(100),
    32.0  = xmod_converter:celsius_to_fahrenheit(0),

    %% Bidirectional: xmod_converter:double_convert calls back to us
    ok = xmod_converter:double_convert(100),

    ok.
