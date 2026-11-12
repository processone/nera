%% Adapted from "Programming Erlang" by Joe Armstrong — area server pattern.
%% Tests RPC-style client/server communication via message passing.
-module(jaerlang_area_server).
-export([test/0, loop/0]).

id(I) -> I.

test() ->
    Pid = spawn(jaerlang_area_server, loop, []),
    %% Rectangle
    12.0 = rpc(Pid, {rectangle, id(3.0), id(4.0)}),
    %% Circle (approximate)
    Area = rpc(Pid, {circle, id(2.0)}),
    true = (Area > 12.5),
    true = (Area < 12.6),
    %% Unknown shape returns {error, Shape}
    {error, {triangle, 3, 4, 5}} = rpc(Pid, {triangle, 3, 4, 5}),
    ok.

rpc(Pid, Request) ->
    Pid ! {self(), Request},
    receive
        {Pid, Response} ->
            Response
    end.

loop() ->
    receive
        {From, {rectangle, Width, Ht}} ->
            From ! {self(), Width * Ht},
            loop();
        {From, {circle, R}} ->
            From ! {self(), 3.14159 * R * R},
            loop();
        {From, Other} ->
            From ! {self(), {error, Other}},
            loop()
    end.
