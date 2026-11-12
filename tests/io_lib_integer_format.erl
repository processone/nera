%% EXPECT: ok
-module(io_lib_integer_format).
-export([test/0]).

test() ->
    "2023-08-07T19:02:24.065432Z" = lists:flatten(io_lib:format(
        "~4..0b-~2..0b-~2..0bT~2..0b:~2..0b:~2..0b.~6..0bZ",
        [2023, 8, 7, 19, 2, 24, 65432])),
    "0000|99|**|00-7|-700|xxx7|00FF|1010" = lists:flatten(io_lib:format(
        "~4..0b|~2..0b|~2..0b|~4..0b|~-4..0B|~4..xb|~4.16.0B|~.2b",
        [0, 99, 123, -7, -7, 7, 255, 10])),
    io:format("ok~n").
