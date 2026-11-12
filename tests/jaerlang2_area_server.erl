%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (area_server_final.erl)
%% Tests: spawn, send/receive, process loop, self(), pid matching in receive
-module(jaerlang2_area_server).
-export([test/0, loop/0]).

test() ->
    Pid = spawn(jaerlang2_area_server, loop, []),

    %% Rectangle
    Pid ! {self(), {rectangle, 3, 4}},
    R1 = receive {Pid, Resp1} -> Resp1 end,
    12 = R1,

    %% Square (as rectangle)
    Pid ! {self(), {rectangle, 5, 5}},
    R2 = receive {Pid, Resp2} -> Resp2 end,
    25 = R2,

    %% Unknown shape
    Pid ! {self(), {hexagon, 5}},
    R3 = receive {Pid, Resp3} -> Resp3 end,
    {error, {hexagon, 5}} = R3,

    ok.

loop() ->
    receive
        {From, {rectangle, Width, Ht}} ->
            From ! {self(), Width * Ht},
            loop();
        {From, {circle, R}} ->
            From ! {self(), 3 * R * R},
            loop();
        {From, Other} ->
            From ! {self(), {error, Other}},
            loop()
    end.
