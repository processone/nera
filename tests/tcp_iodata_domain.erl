-module(tcp_iodata_domain).
-export([test/0]).
test() ->
    {ok, L} = gen_tcp:listen(0, [binary, {active, false}]),
    {ok, Port} = inet:port(L),
    {ok, C} = gen_tcp:connect("127.0.0.1", Port, [binary, {active, false}]),
    {ok, S} = gen_tcp:accept(L),
    lists:foreach(fun(Data) -> {error, badarg} = gen_tcp:send(C, Data) end,
        [[256], [-1], [65, invalid], [65 | 7], 65, invalid, [<<1:1>>]]),
    ok = gen_tcp:send(C, []),
    ok = gen_tcp:send(C, [0, [255, <<"AB">>, []], [67 | <<"D">>]]),
    {ok, <<0,255,65,66,67,68>>} = gen_tcp:recv(S, 6),
    ok = gen_tcp:close(C), {error, _} = gen_tcp:recv(S, 1),
    ok = gen_tcp:close(S), ok = gen_tcp:close(L), ok.
