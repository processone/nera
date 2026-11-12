-module(link_normal).
-export([test/0]).

%% Test: normal exit delivers {'EXIT', Pid, normal} to trapping process
test() ->
    process_flag(trap_exit, true),
    Pid = spawn_link(fun() -> ok end),
    receive
        {'EXIT', Pid, normal} -> ok
    after 1000 ->
        exit(timeout)
    end.
