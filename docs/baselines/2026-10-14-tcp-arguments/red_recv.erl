-module(red_recv).
-export([main/0]).
main() ->
    {ok, L} = gen_tcp:listen(0, [binary, {active, false}]),
    {ok, Port} = inet:port(L),
    {ok, C} = gen_tcp:connect("127.0.0.1", Port, [binary, {active, false}]),
    {ok, S} = gen_tcp:accept(L),
    ok = gen_tcp:send(C, <<42>>),
    {'EXIT', {badarg, _}} = (catch gen_tcp:recv(S, -1)),
    gen_tcp:close(C), gen_tcp:close(S), gen_tcp:close(L).
