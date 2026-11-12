-module(tcp_handle_domain).
-export([test/0]).
test() ->
    lists:foreach(fun invalid/1,
        [0, -1, 4294967295, 8589934591, 1152921504606846976, invalid, []]),
    {ok, L} = gen_tcp:listen(0, [binary, {active, false}]),
    check(L), {ok, Port} = inet:port(L),
    {ok, C} = gen_tcp:connect("127.0.0.1", Port, [binary, {active, false}]),
    {ok, S} = gen_tcp:accept(L), check(C), check(S),
    ok = gen_tcp:send(C, <<42>>), {ok, <<42>>} = gen_tcp:recv(S, 1),
    ok = inet:setopts(S, [{active, once}]),
    ok = gen_tcp:send(C, <<43>>),
    receive {tcp, S, <<43>>} -> ok after 2000 -> error(active_handle_timeout) end,
    ok = gen_tcp:close(S), ok = gen_tcp:close(C), ok = gen_tcp:close(L),
    {error, badarg} = inet:port(L),
    {ok, Replacement} = gen_tcp:listen(0, [binary, {active, false}]),
    check(Replacement), false = L =:= Replacement,
    ok = gen_tcp:close(L), {ok, _} = inet:port(Replacement),
    ok = gen_tcp:close(Replacement), ok.
check(H) when is_integer(H), H > 0, H < 1152921504606846976 -> ok.
invalid(H) ->
    {error, badarg} = inet:port(H),
    {error, closed} = gen_tcp:accept(H),
    {error, enoent} = gen_tcp:recv(H, 1),
    {error, enoent} = gen_tcp:send(H, <<42>>),
    ok = gen_tcp:close(H).
