-module(timer_module).
-export([test/0]).

%% Test the timer stdlib module.

test() ->
    test_sleep(),
    test_send_after(),
    test_tc(),
    test_conversions(),
    ok.

test_sleep() ->
    T0 = erlang:monotonic_time(millisecond),
    timer:sleep(50),
    T1 = erlang:monotonic_time(millisecond),
    Elapsed = T1 - T0,
    true = (Elapsed >= 40),  % allow small jitter
    ok.

test_send_after() ->
    {ok, _TRef} = timer:send_after(50, hello),
    receive
        hello -> ok
    after 500 ->
        error(send_after_timeout)
    end.

test_tc() ->
    {Time, result} = timer:tc(fun() ->
        receive after 20 -> result end
    end),
    true = (Time >= 15000),  % at least 15ms in microseconds
    ok.

test_conversions() ->
    1000 = timer:seconds(1),
    60000 = timer:minutes(1),
    3600000 = timer:hours(1),
    3661000 = timer:hms(1, 1, 1),
    ok.
