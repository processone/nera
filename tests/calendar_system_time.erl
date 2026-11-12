%% EXPECT: ok
%% Regression: calendar:valid_date/1, system_time_to_universal_time/2 and
%% universal_time_to_system_time/1 are missing, so Cowlib's cow_date and
%% cow_cookie fail to build. Expected values were taken from OTP 29 and the
%% same fixture runs natively and on BEAM.
-module(calendar_system_time).
-export([test/0]).
test() ->
    true = calendar:valid_date({2024,2,29}),
    false = calendar:valid_date({2023,2,29}),
    false = calendar:valid_date({1900,2,29}),
    true = calendar:valid_date({2000,2,29}),
    true = calendar:valid_date({2024,12,31}),
    false = calendar:valid_date({2024,0,1}),
    false = calendar:valid_date({2024,13,1}),
    false = calendar:valid_date({2024,4,31}),
    false = calendar:valid_date({2024,1,0}),
    function_clause = failure(fun() -> calendar:valid_date({2024,1.0,1}) end),
    function_clause = failure(fun() -> calendar:valid_date(bad) end),
    function_clause = failure(fun() -> calendar:valid_date({1,2}) end),
    true = apply(calendar, valid_date, [{2024,1,1}]),
    true = (fun calendar:valid_date/1)({2024,1,1}),

    0 = calendar:universal_time_to_system_time({{1970,1,1},{0,0,0}}),
    1700000000 = calendar:universal_time_to_system_time({{2023,11,14},{22,13,20}}),
    -1 = calendar:universal_time_to_system_time({{1969,12,31},{23,59,59}}),
    {{1970,1,1},{0,0,0}} = calendar:system_time_to_universal_time(0, second),
    {{2023,11,14},{22,13,20}} = calendar:system_time_to_universal_time(1700000000, second),
    {{2023,11,14},{22,13,20}} = calendar:system_time_to_universal_time(1700000000999, millisecond),
    {{2023,11,14},{22,13,20}} = calendar:system_time_to_universal_time(1700000000123456, microsecond),
    {{2023,11,14},{22,13,20}} = calendar:system_time_to_universal_time(1700000000999999999, nanosecond),
    {{2023,11,14},{22,13,20}} = calendar:system_time_to_universal_time(17000000009, 10),
    {{1969,12,31},{23,59,59}} = calendar:system_time_to_universal_time(-1, millisecond),
    {{1969,12,31},{23,59,59}} = calendar:system_time_to_universal_time(-1, second),
    {{2024,2,29},{12,0,0}} = calendar:system_time_to_universal_time(
        calendar:universal_time_to_system_time({{2024,2,29},{12,0,0}}), second),
    badarg = failure(fun() -> calendar:system_time_to_universal_time(0, bogus) end),
    badarg = failure(fun() -> calendar:system_time_to_universal_time(0, 0) end),
    badarg = failure(fun() -> calendar:system_time_to_universal_time(1.5, second) end),
    1700000000 = apply(calendar, universal_time_to_system_time, [{{2023,11,14},{22,13,20}}]),
    {{1970,1,1},{0,0,1}} = (fun calendar:system_time_to_universal_time/2)(1, second),
    io:format("ok~n").
failure(F) -> try F() of _ -> unexpected_return catch error:R -> R end.
