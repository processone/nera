%% EXPECT: map_get_bif_ok
%% cow_hpack uses the auto-imported map_get/2 guard BIF.
-module(map_get_bif).
-export([test/0]).

test() ->
    M = #{limit => 3, <<"nested">> => #{x => [a,b]}},
    3 = map_get(limit, M),
    3 = erlang:map_get(limit, M),
    3 = apply(erlang, map_get, [limit, M]),
    F = fun erlang:map_get/2,
    #{x := [a,b]} = F(<<"nested">>, M),
    accepted = allowed(M, 2),
    rejected = allowed(M, 3),
    rejected = allowed(#{}, 0),
    rejected = allowed(not_a_map, 0),
    {badkey, absent} = reason(fun() -> map_get(absent, M) end),
    {badmap, not_a_map} = reason(fun() -> erlang:map_get(limit, not_a_map) end),
    io:format("map_get_bif_ok~n"),
    ok.
allowed(M, N) when N < map_get(limit, M) -> accepted;
allowed(_, _) -> rejected.
reason(F) -> try F() of Value -> {unexpected, Value} catch error:Reason -> Reason end.
