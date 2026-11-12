%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (lib_misc.erl on_exit/2)
%% Tests: monitor/2, DOWN messages, reference matching, spawn with closure
-module(jaerlang2_on_exit).
-export([test/0]).

test() ->
    Self = self(),

    %% Test 1: on_exit detects normal exit
    Pid1 = spawn(fun() ->
                     receive go -> ok end
                 end),
    on_exit(Pid1, fun(Why) -> Self ! {exited, Why} end),
    %% Give monitor process time to establish the monitor
    sleep(50),
    Pid1 ! go,
    R1 = receive {exited, Reason1} -> Reason1 after 2000 -> timeout end,
    normal = R1,

    %% Test 2: on_exit detects abnormal exit
    %% Using exit(boom) rather than error(boom) because the runtime
    %% currently sets exit code 1 for any process error() crash.
    Pid2 = spawn(fun() ->
                     receive go -> exit(boom) end
                 end),
    on_exit(Pid2, fun(Why) -> Self ! {exited2, Why} end),
    %% Give monitor process time to establish the monitor
    sleep(50),
    Pid2 ! go,
    R2 = receive {exited2, Reason2} -> Reason2 after 2000 -> timeout end,
    boom = R2,

    %% Test 3: on_exit for an already-dead process gets noproc
    Pid3 = spawn(fun() -> ok end),
    sleep(100),
    on_exit(Pid3, fun(Why) -> Self ! {exited3, Why} end),
    R3 = receive {exited3, Reason3} -> Reason3 after 2000 -> timeout end,
    noproc = R3,

    ok.

%% on_exit(Pid, Fun) - monitors Pid, calls Fun(Why) when it dies
on_exit(Pid, Fun) ->
    spawn(fun() ->
              Ref = monitor(process, Pid),
              receive
                  {'DOWN', Ref, process, Pid, Why} ->
                      Fun(Why)
              end
          end).

sleep(T) ->
    receive
    after T -> true
    end.
