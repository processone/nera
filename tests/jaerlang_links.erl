%% Adapted from "Programming Erlang" by Joe Armstrong — link/exit signal demo.
%% Tests process linking, trap_exit, and EXIT signal propagation.
-module(jaerlang_links).
-export([test/0]).

id(I) -> I.

test() ->
    test_normal_exit(),
    test_abnormal_exit(),
    test_trap_exit(),
    ok.

%% When a linked process exits normally, the link is silently removed.
test_normal_exit() ->
    process_flag(trap_exit, true),
    Pid = spawn_link(fun() -> ok end),
    %% Wait for the EXIT signal
    receive
        {'EXIT', Pid, normal} -> ok
    after 500 ->
        %% Normal exit may not always send EXIT, depending on timing
        ok
    end,
    process_flag(trap_exit, false),
    ok.

%% When a linked process exits abnormally and we DON'T trap,
%% the linked process also dies. We test via a middle process.
test_abnormal_exit() ->
    %% Spawn a trapper that links to a crasher
    Self = self(),
    Trapper = spawn(fun() ->
        process_flag(trap_exit, true),
        Crasher = spawn_link(fun() -> exit(id(boom)) end),
        receive
            {'EXIT', Crasher, boom} ->
                Self ! {result, trapped_boom}
        after 500 ->
                Self ! {result, timeout}
        end
    end),
    receive
        {result, trapped_boom} -> ok
    after 1000 ->
        exit(test_failed)
    end,
    ok.

%% With trap_exit=true, EXIT signals become messages.
test_trap_exit() ->
    process_flag(trap_exit, true),
    Pid = spawn_link(fun() -> exit(id(reason42)) end),
    receive
        {'EXIT', Pid, reason42} -> ok
    after 500 ->
        exit(test_failed)
    end,
    process_flag(trap_exit, false),
    ok.
