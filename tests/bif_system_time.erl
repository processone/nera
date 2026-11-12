%% Tests erlang:system_time/1.
-module(bif_system_time).
-export([test/0]).

test() ->
    %% system_time(second) returns wall clock seconds
    T1 = erlang:system_time(second),
    true = is_integer(T1),
    %% Should be after year 2020 (1577836800) and before 2100
    true = T1 > 1577836800,
    true = T1 < 4102444800,

    %% Two calls should be monotonically non-decreasing
    T2 = erlang:system_time(second),
    true = T2 >= T1,

    io:format("ok~n"),
    ok.
