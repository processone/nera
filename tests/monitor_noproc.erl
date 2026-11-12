-module(monitor_noproc).
-export([test/0]).

%% Test: monitoring a dead process gets immediate noproc DOWN
test() ->
    Pid = spawn(fun() -> ok end),
    receive after 200 -> ok end,
    Ref = monitor(process, Pid),
    receive
        {'DOWN', Ref, process, Pid, noproc} -> ok
    after 1000 ->
        exit(timeout)
    end.
