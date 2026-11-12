-module(exit_code_spawn_crash).
-export([test/0]).

%% Test that a spawned (unlinked) process crashing with error() does not
%% affect the exit code of the main process. Before the fix, any process
%% calling error() would set the global any_crash flag, causing exit code 1.

id(I) -> I.

test() ->
    %% Spawn an unlinked process that crashes with error()
    spawn(fun() -> error(id(deliberate_crash)) end),
    %% Give it time to crash
    receive after 100 -> ok end,
    %% Main process returns ok — exit code should be 0
    ok.
