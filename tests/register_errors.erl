-module(register_errors).
-export([test/0]).

%% Test: error cases for register/2 — all should raise badarg
test() ->
    Self = self(),

    %% 1. Double-register the same name
    true = register(err_name, Self),
    Pid2 = spawn(fun() -> receive stop -> ok end end),
    badarg = try register(err_name, Pid2)
             catch error:badarg -> badarg
             end,

    %% 2. Register a PID that is already registered under a different name
    badarg = try register(err_other, Self)
             catch error:badarg -> badarg
             end,

    %% Cleanup first registration
    unregister(err_name),

    %% 3. Register a dead process
    Pid3 = spawn(fun() -> ok end),
    timer_sleep(50),  %% let it exit
    badarg = try register(dead_proc, Pid3)
             catch error:badarg -> badarg
             end,

    %% Cleanup
    Pid2 ! stop,

    ok.

timer_sleep(Ms) ->
    receive
    after Ms -> ok
    end.
