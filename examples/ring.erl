-module(ring).
-export([start/0, node_loop/1]).

start() ->
    N = 100,
    M = 10,
    First = self(),
    Last = create_ring(N, First),
    Last ! M,
    receive
        0 -> io:format("Ring complete~n")
    end.

create_ring(0, Prev) -> Prev;
create_ring(I, Prev) ->
    Pid = spawn(ring, node_loop, [Prev]),
    create_ring(I - 1, Pid).

node_loop(Next) ->
    receive
        0 ->
            Next ! 0;
        M ->
            Next ! M - 1,
            node_loop(Next)
    end.
