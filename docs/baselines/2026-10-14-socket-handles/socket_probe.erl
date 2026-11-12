-module(socket_probe).
-export([main/0]).
main() ->
    {Registration, ok} = timer:tc(fun() -> register_loop(1000) end),
    {ok, L} = gen_tcp:listen(0, [binary, {active, false}]),
    {ok, Port} = inet:port(L),
    {ok, C} = gen_tcp:connect("127.0.0.1", Port, [binary, {active, false}]),
    {ok, S} = gen_tcp:accept(L),
    {Roundtrip, ok} = timer:tc(fun() -> roundtrip(C, S, 2000) end),
    ok = gen_tcp:close(S), ok = gen_tcp:close(C), ok = gen_tcp:close(L),
    io:format("registration ~p~nroundtrip ~p~n", [Registration, Roundtrip]).
register_loop(0) -> ok;
register_loop(N) ->
    {ok, S} = gen_tcp:listen(0, [binary, {active, false}]),
    ok = gen_tcp:close(S), register_loop(N - 1).
roundtrip(_, _, 0) -> ok;
roundtrip(C, S, N) ->
    ok = gen_tcp:send(C, <<42>>), {ok, <<42>>} = gen_tcp:recv(S, 1),
    ok = gen_tcp:send(S, <<43>>), {ok, <<43>>} = gen_tcp:recv(C, 1),
    roundtrip(C, S, N - 1).
