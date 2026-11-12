-module(unlink_basic).
-export([test/0]).

%% Test: unlink prevents EXIT delivery
test() ->
    process_flag(trap_exit, true),
    Pid = spawn_link(fun() -> receive after 100 -> exit(boom) end end),
    unlink(Pid),
    receive
        {'EXIT', _, _} -> exit(should_not_receive)
    after 500 ->
        ok
    end.
