-module(register_reuse).
-export([test/0]).

%% Test: re-registering a name after the original process dies
test() ->
    %% Register a short-lived process
    Pid1 = spawn(fun() -> ok end),
    timer_sleep(50),  %% let it exit

    %% Name should be auto-cleaned
    undefined = whereis(reuse_name),

    %% Now register a new process under the same name
    Self = self(),
    true = register(reuse_name, Self),
    Self = whereis(reuse_name),

    %% Unregister and re-register with yet another process
    unregister(reuse_name),
    Pid2 = spawn(fun() -> receive stop -> ok end end),
    true = register(reuse_name, Pid2),
    Pid2 = whereis(reuse_name),

    %% Cleanup
    reuse_name ! stop,
    timer_sleep(50),
    undefined = whereis(reuse_name),

    ok.

timer_sleep(Ms) ->
    receive
    after Ms -> ok
    end.
