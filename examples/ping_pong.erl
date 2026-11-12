-module(ping_pong).
-export([start/0, pong/0]).

start() ->
    Pong = spawn(ping_pong, pong, []),
    ping(Pong, 10).

ping(_, 0) ->
    io:format("Ping done~n");
ping(Pong, N) ->
    Pong ! {ping, self()},
    receive
        pong ->
            io:format("Ping received pong (~w remaining)~n", [N]),
            ping(Pong, N - 1)
    end.

pong() ->
    receive
        {ping, From} ->
            io:format("Pong received ping~n"),
            From ! pong,
            pong()
    end.
