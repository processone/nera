%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (stimer.erl)
%% Tests: spawn, receive...after, message cancellation
-module(jaerlang2_stimer).
-export([test/0]).

test() ->
    %% Test 1: timer fires and sends us a message
    Self = self(),
    _T1 = spawn(fun() -> timer(50, fun() -> Self ! timer_fired end) end),
    R1 = receive
        timer_fired -> ok
    after 2000 ->
        timeout
    end,
    ok = R1,

    %% Test 2: timer is cancelled before firing
    T2 = spawn(fun() -> timer(2000, fun() -> Self ! should_not_fire end) end),
    T2 ! cancel,
    R2 = receive
        should_not_fire -> bad
    after 200 ->
        ok
    end,
    ok = R2,

    ok.

timer(Time, Fun) ->
    receive
        cancel ->
            void
    after Time ->
        Fun()
    end.
