%% EXPECT: lists_suffix_ok
-module(lists_suffix).
-export([test/0]).
test() ->
    true = lists:suffix([], []), true = lists:suffix([], [a]),
    true = lists:suffix("_test", "sample_test"),
    false = lists:suffix("_test", "sample_test_"),
    false = lists:suffix([a,b], [b]),
    false = lists:suffix([1], [1.0]),
    ok = try lists:suffix([], invalid) of _ -> error(unexpected) catch error:badarg -> ok end,
    io:format("lists_suffix_ok~n").
