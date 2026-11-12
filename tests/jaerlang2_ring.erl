%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (Ch. 12 exercises)
%% Tests: spawn N processes in a ring, message passing throughput, tail-recursive loops
-module(jaerlang2_ring).
-export([test/0]).

test() ->
    %% Ring of 10 processes, 5 laps
    hello = ring(10, 5, hello),

    %% Ring of 1 process, 3 laps (degenerate case)
    ping = ring(1, 3, ping),

    %% Ring of 3 processes, 1 lap
    42 = ring(3, 1, 42),

    %% Larger ring: 20 processes, 10 laps = 200 hops
    done = ring(20, 10, done),

    ok.

%% Create N processes in a ring: self -> P1 -> P2 -> ... -> PN -> self
%% Send Token around the ring M times, then return it.
ring(N, M, Token) ->
    First = start_ring(N, self()),
    send_laps(First, Token, M).

start_ring(1, Next) ->
    spawn(fun() -> ring_node(Next) end);
start_ring(N, Next) ->
    Pid = spawn(fun() -> ring_node(Next) end),
    start_ring(N - 1, Pid).

ring_node(Next) ->
    receive
        Msg ->
            Next ! Msg,
            ring_node(Next)
    end.

send_laps(_, Token, 0) -> Token;
send_laps(First, Token, M) ->
    First ! Token,
    receive
        _ -> send_laps(First, Token, M - 1)
    end.
