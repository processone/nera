%% Run OTP assertion macros natively; this is not the EUnit test runner.
-module(assertion_probe).
-include_lib("stdlib/include/assert.hrl").
-export([test/0]).
test() ->
    ok = ?assertEqual(42, 40 + 2),
    Diagnostic = try ?assertEqual(43, 40 + 2) of
        _ -> missing_failure
    catch error:{assertEqual, Details} -> Details end,
    "40 + 2" = proplists:get_value(expression, Diagnostic),
    43 = proplists:get_value(expected, Diagnostic),
    42 = proplists:get_value(value, Diagnostic),
    io:format("OTP_ASSERTIONS_OK~n").
