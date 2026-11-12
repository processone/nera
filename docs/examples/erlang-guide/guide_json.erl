-module(guide_json).
-export([start/0]).

start() ->
    Input = <<"{\"language\":\"Erlang\",\"native\":true,\"numbers\":[1,2,3]}">>,
    {ok, Data} = thoas:decode(Input),
    <<"Erlang">> = maps:get(<<"language">>, Data),
    true = maps:get(<<"native">>, Data),
    [1, 2, 3] = maps:get(<<"numbers">>, Data),
    Encoded = thoas:encode(Data),
    {ok, Data} = thoas:decode(Encoded),
    io:format("Thoas decoded Erlang, native=true, numbers=[1,2,3]~n"),
    io:format("JSON roundtrip: ok~n").
