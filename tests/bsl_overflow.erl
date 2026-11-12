%% EXPECT: bsl_overflow ok
-module(bsl_overflow).
-export([test/0, shift/2]).
shift(Value, Amount) -> Value bsl Amount.
test() ->
    lists:foreach(fun({Value, Amount, Decimal}) ->
        Expected = list_to_integer(Decimal),
        Expected = shift(Value, Amount)
    end, [{16,60,"18446744073709551616"},
          {-16,60,"-18446744073709551616"},
          {17,60,"19599665578316398592"},
          {1152921504606846975,5,"36893488147419103200"},
          {-1152921504606846976,4,"-18446744073709551616"},
          {1,59,"576460752303423488"}, {1,60,"1152921504606846976"},
          {-1,60,"-1152921504606846976"}, {0,60,"0"}, {1,63,"9223372036854775808"},
          {-1,63,"-9223372036854775808"}, {1,64,"18446744073709551616"},
          {8,-2,"2"}, {-8,-2,"-2"}, {7,0,"7"}]),
    lists:foreach(fun(Value) -> sweep(Value, 0, Value) end,
        [-1152921504606846976, -1152921504606846975, -17, -16, -1,
         0, 1, 16, 17, 1152921504606846974, 1152921504606846975]),
    io:format("bsl_overflow ok~n"), ok.
sweep(_, 65, _) -> ok;
sweep(Value, Amount, Expected) ->
    Expected = shift(Value, Amount),
    %% Repeated addition is independent of the shift implementation.
    sweep(Value, Amount + 1, Expected + Expected).
