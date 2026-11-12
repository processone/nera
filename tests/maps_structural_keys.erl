%% EXPECT: maps_structural_keys_ok
-module(maps_structural_keys).
-export([test/0]).
test() ->
    Key = lists:reverse([3,2,1]),
    M = maps:put(Key, value, #{}),
    value = maps:get([1,2,3], M),
    {ok,value} = maps:find([1,2,3], M),
    true = maps:is_key([1,2,3], M),
    false = maps:is_key([1,2,3.0], M),
    1 = maps:size(maps:put([1,2,3], updated, M)),
    0 = maps:size(maps:remove([1,2,3], M)),
    T = maps:put({[1],#{key => [2]}}, value, #{}),
    value = maps:get({[1],#{key => [2]}}, T),
    false = maps:is_key({[1.0],#{key => [2]}}, T),
    false = maps:is_key({[1],#{key => [2.0]}}, T),
    io:format("maps_structural_keys_ok~n").
