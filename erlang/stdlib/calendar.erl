%% Project-owned proleptic Gregorian arithmetic, including astronomical year 0.
%% No OTP implementation is vendored. A 400-year cycle has 146097 days.
-module(calendar).
-export([universal_time/0, day_of_the_week/1, datetime_to_gregorian_seconds/1,
         gregorian_seconds_to_datetime/1, valid_date/1,
         system_time_to_universal_time/2, universal_time_to_system_time/1]).

-define(SECONDS_FROM_0_TO_1970, 62167219200).

universal_time() ->
    erlang:universaltime().

%% As OTP: a date that is not a tuple of three integers has no matching
%% clause and raises function_clause.
valid_date({Y, M, D}) when is_integer(Y), is_integer(M), is_integer(D) ->
    M > 0 andalso M < 13 andalso D > 0 andalso D =< month_length(Y, M).

%% Floors toward negative infinity, as erlang:convert_time_unit/3 does.
system_time_to_universal_time(Time, Unit) when is_integer(Time) ->
    Secs = floor_div(Time, units_per_second(Unit)),
    gregorian_seconds_to_datetime(Secs + ?SECONDS_FROM_0_TO_1970);
system_time_to_universal_time(_, _) ->
    erlang:error(badarg).

universal_time_to_system_time(DateTime) ->
    datetime_to_gregorian_seconds(DateTime) - ?SECONDS_FROM_0_TO_1970.

units_per_second(second) -> 1;
units_per_second(millisecond) -> 1000;
units_per_second(microsecond) -> 1000000;
units_per_second(nanosecond) -> 1000000000;
units_per_second(native) -> 1000000000;
units_per_second(N) when is_integer(N), N > 0 -> N;
units_per_second(_) -> erlang:error(badarg).

day_of_the_week(Date) ->
    mod(days(Date)+5, 7)+1.

datetime_to_gregorian_seconds({Date, {H,M,S}})
        when is_integer(H), H >= 0, H < 24,
             is_integer(M), M >= 0, M < 60,
             is_integer(S), S >= 0, S < 60 ->
    days(Date)*86400 + H*3600 + M*60 + S.

gregorian_seconds_to_datetime(Seconds) ->
    Day = floor_div(Seconds, 86400),
    Time = Seconds - Day*86400,
    Era = floor_div(Day, 146097),
    BaseYear = Era*400,
    Year = find_year(Day, BaseYear, BaseYear+400),
    Date = find_month(Year, Day-year_start(Year), 1),
    {Date, {Time div 3600, (Time rem 3600) div 60, Time rem 60}}.

%% Normalize months arithmetically; positive days can overflow the month,
%% as with the OTP 29 conversion routines (these are not date validators).
days({Year,Month,Day}) when is_integer(Day), Day > 0 ->
    Shift = floor_div(Month-1, 12),
    Y = Year+Shift,
    M = Month-Shift*12,
    year_start(Y) + month_start(Y, M, 1, 0) + Day-1.

year_start(Y) ->
    365*Y + floor_div(Y+3,4) - floor_div(Y+99,100) + floor_div(Y+399,400).

month_start(_, M, M, Acc) -> Acc;
month_start(Y, M, Current, Acc) ->
    month_start(Y, M, Current+1, Acc+month_length(Y,Current)).

find_year(_, Low, High) when High-Low =:= 1 -> Low;
find_year(Day, Low, High) ->
    Mid = floor_div(Low+High, 2),
    case year_start(Mid) =< Day of
        true -> find_year(Day, Mid, High);
        false -> find_year(Day, Low, Mid)
    end.

find_month(Y, Day, M) ->
    Length = month_length(Y,M),
    case Day < Length of
        true -> {Y,M,Day+1};
        false -> find_month(Y,Day-Length,M+1)
    end.

month_length(Y,2) ->
    case Y rem 4 =:= 0 andalso (Y rem 100 =/= 0 orelse Y rem 400 =:= 0) of
        true -> 29;
        false -> 28
    end;
month_length(_,M) when M =:= 4; M =:= 6; M =:= 9; M =:= 11 -> 30;
month_length(_,_) -> 31.

%% Erlang div truncates toward zero; calendar eras need mathematical floor.
floor_div(A,B) ->
    Q = A div B,
    case A rem B < 0 of true -> Q-1; false -> Q end.
mod(A,B) -> A-floor_div(A,B)*B.
