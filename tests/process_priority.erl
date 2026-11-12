%% EXPECT: process_priority_ok
-module(process_priority).
-export([test/0]).
test() ->
    normal = process_flag(priority, low),
    {priority, low} = process_info(self(), priority),
    low = process_flag(priority, normal),
    {priority, normal} = process_info(self(), priority),
    ok = try process_flag(priority, invalid) of _ -> error(unexpected) catch error:badarg -> ok end,
    normal = process_flag(priority, normal),
    io:format("process_priority_ok~n").
