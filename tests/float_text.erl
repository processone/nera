%% EXPECT: 9.007199254740992e15|1.0e16|-0.0|1.0e-5 float_text_ok
%% Regression for BEAM-compatible term float text and float_to_list/1,2.
%% Expected text was read from OTP 29; run unchanged natively and on BEAM.
-module(float_text).
-export([test/0]).

test() ->
    %% Both term directives use shortest round-tripping digits, fixed on
    %% notation-length ties, and scientific notation at and above 2^53.
    lists:foreach(fun({V, Text}) ->
        Text = f("~w", [V]),
        Text = f("~p", [V]),
        Text = float_to_list(id(V), [short]),
        Text = apply(erlang, id(float_to_list), [V, [short]]),
        Convert = fun erlang:float_to_list/2,
        Text = Convert(V, [short])
    end, [{0.0, "0.0"}, {-0.0, "-0.0"},
          {100.0, "100.0"}, {1000.0, "1.0e3"},
          {1.0e15, "1.0e15"},
          {1234567890123456.0, "1234567890123456.0"},
          {9007199254740991.0, "9007199254740991.0"},
          {9007199254740992.0, "9.007199254740992e15"},
          {1.0e16, "1.0e16"}, {1.0e21, "1.0e21"},
          {576460752303423488.0, "5.764607523034235e17"},
          {-9007199254740992.0, "-9.007199254740992e15"},
          {0.0001, "0.0001"}, {0.00001, "1.0e-5"},
          {1.234e-4, "1.234e-4"},
          {0.1 + 0.2, "0.30000000000000004"},
          {5.0e-324, "5.0e-324"},
          {2.2250738585072014e-308, "2.2250738585072014e-308"},
          {1.7976931348623157e308, "1.7976931348623157e308"}]),
    %% Legacy conversion retains twenty decimals and a signed exponent
    %% padded to at least two digits, unlike shortest formatting.
    lists:foreach(fun({V, Text}) ->
        Text = float_to_list(id(V)),
        Text = float_to_list(id(V), []),
        Text = apply(erlang, id(float_to_list), [V])
    end, [{9007199254740992.0, "9.00719925474099200000e+15"},
          {0.001, "1.00000000000000002082e-03"},
          {0.0, "0.00000000000000000000e+00"},
          {-0.0, "-0.00000000000000000000e+00"},
          {1.0e100, "1.00000000000000001590e+100"}]),
    lists:foreach(fun({V, Options, Text}) ->
        Text = float_to_list(id(V), id(Options))
    end, [{1.25, [{scientific, 2}], "1.25e+00"},
          {1.25, [{scientific, 0}], "1e+00"},
          {1.0, [{scientific, -1}], "1.000000e+00"},
          {0.001, [{scientific, 3}], "1.000e-03"},
          {1.25, [{decimals, 1}], "1.3"},
          {1.75, [{decimals, 1}], "1.8"},
          {1.25, [{decimals, 0}], "1"},
          {1.25, [{decimals, 4}, compact], "1.25"},
          {1.0, [compact, {decimals, 4}], "1.0"},
          {-0.0, [{decimals, 2}, compact], "-0.0"},
          {1.0, [{scientific, 2}, compact], "1.00e+00"},
          {1.0, [short, {scientific, 3}], "1.000e+00"},
          {1.0, [{scientific, 3}, short], "1.0"},
          {1.25, [{scientific, 2}, {decimals, 3}], "1.250"},
          {1.25, [{decimals, 2}, {scientific, 3}], "1.250e+00"},
          {1.0, [{decimals, -1}, short], "1.0"}]),
    %% Precision and total output limits, including sign and exponent bytes.
    255 = length(float_to_list(1.25, [{decimals, 253}])),
    255 = length(float_to_list(1.25, [{scientific, 249}])),
    lists:foreach(fun(Options) ->
        badarg = failure(fun() -> float_to_list(id(1.0), Options) end)
    end, [bad, [short | bad], [unknown], [1], [{decimals, -1}],
          [{decimals, 254}], [{scientific, 250}],
          [{decimals, 1.0}], [{decimals, 2, extra}]]),
    badarg = failure(fun() -> float_to_list(id(-1.25), [{decimals, 253}]) end),
    badarg = failure(fun() -> float_to_list(id(1.0e100), [{decimals, 200}]) end),
    badarg = failure(fun() -> float_to_list(id(1), [short]) end),
    badarg = failure(fun() -> float_to_list(id(1)) end),
    "{9.007199254740992e15,[1.0e16]}" =
        f("~w", [{9007199254740992.0, [1.0e16]}]),
    ok = io:format("~w|~p|~w|~p float_text_ok~n",
                   [9007199254740992.0, 1.0e16, -0.0, 0.00001]).

f(Format, Args) -> lists:flatten(io_lib:format(Format, Args)).
failure(Fun) ->
    try Fun() of Value -> {unexpected, Value}
    catch error:Reason -> Reason end.
id(V) -> V.
