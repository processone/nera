-module(recv_probe).
-export([main/0]).
main() ->
    lists:foreach(fun({Size, Count}) -> bench(Size, Count) end,
        [{1, 2000}, {64, 2000}, {4096, 1000}, {1048576, 64}]).
bench(Size, Count) ->
    {ok, L} = gen_tcp:listen(0, [binary, {active, false}]),
    {ok, Port} = inet:port(L),
    {ok, C} = gen_tcp:connect("127.0.0.1", Port, [binary, {active, false}]),
    {ok, S} = gen_tcp:accept(L),
    Payload = binary:copy(<<42>>, Size),
    Parent = self(),
    spawn(fun() -> receive go -> send_loop(C, S, Payload, Count + 8), Parent ! done end end) ! go,
    recv_loop(S, C, Payload, Size, 8),
    {Elapsed, ok} = timer:tc(fun() -> recv_loop(S, C, Payload, Size, Count) end),
    receive done -> ok end,
    io:format("recv_~p ~p~n", [Size, Elapsed]),
    gen_tcp:close(C), gen_tcp:close(S), gen_tcp:close(L).
send_loop(_, _, _, 0) -> ok;
send_loop(C, S, Payload, N) ->
    ok = gen_tcp:send(C, Payload), {ok, <<1>>} = gen_tcp:recv(C, 1, infinity),
    send_loop(C, S, Payload, N - 1).
recv_loop(_, _, _, _, 0) -> ok;
recv_loop(S, C, Payload, Size, N) ->
    {ok, Payload} = gen_tcp:recv(S, Size, infinity), ok = gen_tcp:send(S, <<1>>),
    recv_loop(S, C, Payload, Size, N - 1).
