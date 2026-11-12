-module(register_registered).
-export([test/0]).

%% Test: registered/0 returns the list of all registered process names
test() ->
    %% Start clean — register two names
    Self = self(),
    true = register(reg_a, Self),

    Pid2 = spawn(fun() -> receive stop -> ok end end),
    true = register(reg_b, Pid2),

    %% registered/0 should contain both names
    Names = registered(),
    true = is_list(Names),
    true = lists:member(reg_a, Names),
    true = lists:member(reg_b, Names),

    %% Cleanup
    unregister(reg_a),
    reg_b ! stop,
    timer_sleep(50),

    %% After cleanup, neither should appear
    Names2 = registered(),
    false = lists:member(reg_a, Names2),
    false = lists:member(reg_b, Names2),

    ok.

timer_sleep(Ms) ->
    receive
    after Ms -> ok
    end.
