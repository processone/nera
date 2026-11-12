-module(demonitor_basic).
-export([test/0]).

%% Test: demonitor prevents DOWN delivery
test() ->
    Pid = spawn(fun() -> receive after 100 -> exit(boom) end end),
    Ref = monitor(process, Pid),
    demonitor(Ref),
    receive
        {'DOWN', _, _, _, _} -> exit(should_not_receive)
    after 500 ->
        ok
    end.
