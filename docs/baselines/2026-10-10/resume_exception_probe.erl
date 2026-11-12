%% Both engines must print before and after. Nera currently prints only before
%% and exits with status 0. The wait forces suspension before raising the exit.
-module(resume_exception_probe).
-export([start/0]).

start() ->
    io:format("before~n"),
    {'EXIT', timeout} = (catch wait()),
    io:format("after~n").

wait() ->
    receive
        never -> ok
    after 10 ->
        exit(timeout)
    end.
