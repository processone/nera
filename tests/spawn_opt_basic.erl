-module(spawn_opt_basic).
-export([test/0]).

test() ->
    %% Test spawn_opt/2 with link
    process_flag(trap_exit, true),
    Pid1 = spawn_opt(fun() -> ok end, [link]),
    receive
        {'EXIT', Pid1, normal} -> ok
    after 2000 ->
        error(link_timeout)
    end,

    %% Test spawn_opt/2 with monitor
    {Pid2, Ref} = spawn_opt(fun() -> ok end, [monitor]),
    true = is_pid(Pid2),
    true = is_reference(Ref),
    receive
        {'DOWN', Ref, process, Pid2, normal} -> ok
    after 2000 ->
        error(monitor_timeout)
    end,

    %% Test spawn_opt/2 with fullsweep_after
    Pid3 = spawn_opt(fun() ->
        0 = process_flag(fullsweep_after, 100),
        ok
    end, [{fullsweep_after, 0}]),
    timer:sleep(100),
    true = is_pid(Pid3),

    %% Test spawn_opt/2 with max_heap_size + link
    Pid4 = spawn_opt(fun() -> grow([]) end, [link, {max_heap_size, 500}]),
    receive
        {'EXIT', Pid4, killed} -> ok
    after 5000 ->
        error(max_heap_timeout)
    end,

    ok.

grow(Acc) ->
    grow([lists:seq(1, 50) | Acc]).
