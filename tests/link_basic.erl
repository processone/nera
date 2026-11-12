-module(link_basic).
-export([test/0]).

%% Test: trap_exit + spawn_link + receive EXIT message
test() ->
    process_flag(trap_exit, true),
    Pid = spawn_link(fun() -> exit(boom) end),
    receive
        {'EXIT', Pid, boom} -> ok
    after 1000 ->
        exit(timeout)
    end.
