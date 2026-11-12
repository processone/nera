-module(echo).
-export([start/0, server/0]).

%% Simple echo server: a client spawns a server, sends 3 messages,
%% and prints the echoed replies.

start() ->
    Server = spawn(echo, server, []),
    Self = self(),
    Server ! {echo, Self, hello},
    receive
        {reply, hello} ->
            io:format("Got echo: hello~n")
    end,
    Server ! {echo, Self, world},
    receive
        {reply, world} ->
            io:format("Got echo: world~n")
    end,
    Server ! {echo, Self, done},
    receive
        {reply, done} ->
            io:format("Got echo: done~n")
    end,
    Server ! stop,
    io:format("Echo test passed~n").

server() ->
    receive
        {echo, From, Msg} ->
            From ! {reply, Msg},
            server();
        stop ->
            ok
    end.
