-module(guide_links).
-export([start/0]).

start() ->
    process_flag(trap_exit, true),
    Parent = self(),
    Worker = spawn_link(fun() -> worker(Parent) end),
    Ref = make_ref(),
    Worker ! {square, Ref, 12},
    receive
        {result, Ref, Value} ->
            144 = Value,
            io:format("Worker replied: ~p~n", [Value])
    after 5000 ->
        error(reply_timeout)
    end,
    Worker ! stop,
    receive
        {'EXIT', Worker, finished} ->
            io:format("Linked worker exited: finished~n")
    after 5000 ->
        error(exit_timeout)
    end.

worker(Parent) ->
    receive
        {square, Ref, N} ->
            Parent ! {result, Ref, N * N},
            worker(Parent);
        stop ->
            exit(finished)
    end.
