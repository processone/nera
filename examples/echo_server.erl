-module(echo_server).
-export([start/0, accept_loop/1, echo_loop/1]).

start() ->
    {ok, LSock} = gen_tcp:listen(4000, []),
    io:format("Listening on port 4000~n", []),
    accept_loop(LSock).

accept_loop(LSock) ->
    {ok, Sock} = gen_tcp:accept(LSock),
    io:format("Client connected~n", []),
    spawn(echo_server, echo_loop, [Sock]),
    accept_loop(LSock).

echo_loop(Sock) ->
    case gen_tcp:recv(Sock, 0) of
        {ok, Data} ->
            gen_tcp:send(Sock, Data),
            echo_loop(Sock);
        {error, closed} ->
            io:format("Client disconnected~n", []),
            gen_tcp:close(Sock),
            ok
    end.
