-module(monitor_basic).
-export([test/0]).

%% Test: monitor + receive DOWN message
test() ->
    Pid = spawn(fun() -> exit(reason123) end),
    Ref = monitor(process, Pid),
    receive
        {'DOWN', Ref, process, Pid, reason123} -> ok
    after 1000 ->
        exit(timeout)
    end.
