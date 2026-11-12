%% EXPECT: eunit_collections_ok
-module(eunit_collections).
-export([test/0]).
test() ->
    D0 = dict:new(),
    0 = dict:size(D0), error = dict:find(missing, D0),
    D = dict:from_list([{a, 1}, {a, 2}, {1, integer}, {1.0, float}]),
    3 = dict:size(D), {ok, 2} = dict:find(a, D),
    {ok, integer} = dict:find(1, D), {ok, float} = dict:find(1.0, D),
    2 = dict:size(dict:erase(a, D)),
    [{a, 3}] = dict:to_list(dict:store(a, 3, D0)),
    S = sets:from_list([a, b, a, 1, 1.0]),
    4 = sets:size(S), true = sets:is_element(1.0, S),
    false = sets:is_element(c, S),
    3 = sets:size(sets:del_element(a, S)),
    5 = sets:size(sets:union(S, sets:from_list([c, a]))),
    5 = sets:size(sets:add_element(c, S)),
    4 = sets:fold(fun(_, N) -> N + 1 end, 0, S),
    0 = sets:size(sets:new()),
    4 = length(sets:to_list(S)),
    io:format("eunit_collections_ok~n").
