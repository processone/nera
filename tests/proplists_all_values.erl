%% EXPECT: proplists_all_values_ok
-module(proplists_all_values).
-export([test/0]).
test() ->
    [] = proplists:get_all_values(key, []),
    [first, true, second, true] = proplists:get_all_values(key,
        [{key, first}, other, key, {key, second}, {key}, {key, a, b}, key]),
    [one] = proplists:get_all_values(1, [1, {1, one}, {1.0, wrong}]),
    [] = proplists:get_all_values(key, [#{key => value}, {other, value}]),
    io:format("proplists_all_values_ok~n").
