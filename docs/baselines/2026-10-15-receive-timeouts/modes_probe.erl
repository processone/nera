-module(modes_probe).
-export([main/0]).
main() ->
    bench(raw_once, raw, once, <<42>>),
    bench(line_once, line, once, <<42,10>>),
    bench(line_active, line, true, <<42,10>>).
bench(Name, Packet, Active, Payload) ->
    {ok, L} = gen_tcp:listen(0, [binary, {active, false}, {packet, Packet}]),
    {ok, Port} = inet:port(L),
    {ok, C} = gen_tcp:connect("127.0.0.1", Port, [binary, {active, false}]),
    {ok, S} = gen_tcp:accept(L),
    case Active of true -> ok = inet:setopts(S, [{active, true}]); _ -> ok end,
    Expected = case Packet of line -> <<42>>; raw -> Payload end,
    loop(C, S, Payload, Expected, Active, 16),
    {Time, ok} = timer:tc(fun() -> loop(C, S, Payload, Expected, Active, 2000) end),
    io:format("~p ~p~n", [Name, Time]),
    gen_tcp:close(C), gen_tcp:close(S), gen_tcp:close(L).
loop(_, _, _, _, _, 0) -> ok;
loop(C, S, Payload, Expected, Active, N) ->
    case Active of once -> ok = inet:setopts(S, [{active, once}]); _ -> ok end,
    ok = gen_tcp:send(C, Payload),
    receive {tcp, S, Expected} -> ok after 5000 -> error(mode_timeout) end,
    loop(C, S, Payload, Expected, Active, N - 1).
