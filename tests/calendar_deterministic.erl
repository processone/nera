%% EXPECT: ok
-module(calendar_deterministic).
-export([test/0]).
test() ->
    6 = calendar:day_of_the_week({0,1,1}),
    1 = calendar:day_of_the_week({1,1,1}),
    4 = calendar:day_of_the_week({1970,1,1}),
    2 = calendar:day_of_the_week({2000,2,29}),
    0 = calendar:datetime_to_gregorian_seconds({{0,1,1},{0,0,0}}),
    62167219200 = calendar:datetime_to_gregorian_seconds({{1970,1,1},{0,0,0}}),
    {{-1,12,31},{23,59,59}} = calendar:gregorian_seconds_to_datetime(-1),
    -12622780800 = calendar:datetime_to_gregorian_seconds({{-400,1,1},{0,0,0}}),
    {{1900,3,1},{0,0,0}} = calendar:gregorian_seconds_to_datetime(
        calendar:datetime_to_gregorian_seconds({{1900,2,29},{0,0,0}})),
    {{2024,1,1},{0,0,0}} = calendar:gregorian_seconds_to_datetime(
        calendar:datetime_to_gregorian_seconds({{2023,13,1},{0,0,0}})),
    lists:foreach(fun(Y) ->
        Days = case Y rem 4 =:= 0 andalso (Y rem 100 =/= 0 orelse Y rem 400 =:= 0) of true -> 29; false -> 28 end,
        A = calendar:datetime_to_gregorian_seconds({{Y,2,Days},{23,59,59}}),
        {{Y,3,1},{0,0,0}} = calendar:gregorian_seconds_to_datetime(A+1),
        B = calendar:datetime_to_gregorian_seconds({{Y,12,31},{23,59,59}}),
        Next = Y+1,
        {{Next,1,1},{0,0,0}} = calendar:gregorian_seconds_to_datetime(B+1)
    end, [-401,-400,-100,-4,-1,0,1,4,100,400,1582,1600,1700,1900,2000,2024,2100,2400,10000]),
    lists:foreach(fun(S) ->
        S = calendar:datetime_to_gregorian_seconds(calendar:gregorian_seconds_to_datetime(S))
    end, [-(10000000000*10000000000), -86401,-86400,-1,0,1,86399,86400,10000000000*10000000000]),
    function_clause = failure(fun() -> calendar:day_of_the_week({2024,1,0}) end),
    function_clause = failure(fun() -> calendar:datetime_to_gregorian_seconds({{2024,1,1},{24,0,0}}) end),
    function_clause = failure(fun() -> calendar:datetime_to_gregorian_seconds(bad) end),
    badarith = failure(fun() -> calendar:gregorian_seconds_to_datetime(1.5) end),
    F = fun calendar:day_of_the_week/1,
    4 = F({1970,1,1}),
    4 = apply(calendar, day_of_the_week, [{1970,1,1}]),
    %% Rolling date checksum independently evaluated with OTP 29.0.5.
    998368685 = cycle(0, 146097, 0),
    io:format("ok~n").
cycle(D, D, Hash) -> Hash;
cycle(D, End, Hash) ->
    S = D*86400+43201,
    {Date,{12,0,1}} = calendar:gregorian_seconds_to_datetime(S),
    S = calendar:datetime_to_gregorian_seconds({Date,{12,0,1}}),
    Weekday = (D+5) rem 7+1,
    Weekday = calendar:day_of_the_week(Date),
    {Y,M,Day} = Date,
    cycle(D+1, End, (Hash*31+Y*10000+M*100+Day) rem 1000000007).
failure(F) -> try F() of _ -> unexpected_return catch error:R -> R end.
