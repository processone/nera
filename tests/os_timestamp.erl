-module(os_timestamp).
-export([test/0]).
test() ->
    Before = erlang:system_time(microsecond),
    {Mega, Seconds, Micro} = os:timestamp(),
    After = erlang:system_time(microsecond),
    true = is_integer(Mega) andalso Mega >= 0,
    true = is_integer(Seconds) andalso Seconds >= 0 andalso Seconds < 1000000,
    true = is_integer(Micro) andalso Micro >= 0 andalso Micro < 1000000,
    Value = (Mega * 1000000 + Seconds) * 1000000 + Micro,
    %% OTP time correction may differ slightly from the raw operating-system clock.
    true = abs(Value - Before) < 5000000,
    true = abs(Value - After) < 5000000,
    ok.
