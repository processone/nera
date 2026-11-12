%% Test that a message arriving during `receive after` cancels the timer
%% and delivers the message promptly, without waiting for the full timeout.
-module(timer_cancel_on_msg).
-export([test/0]).

test() ->
    Self = self(),

    %% Spawn a worker that waits in receive after 5000.
    %% If the timer is NOT cancelled, this would take 5 seconds.
    Worker = spawn(fun() ->
                       Result = receive
                                    {ping, Val} -> {pong, Val}
                                after 5000 ->
                                    timeout
                                end,
                       Self ! {worker_result, Result}
                   end),

    %% Give the worker a moment to enter the receive.
    receive after 20 -> ok end,

    %% Send a message — this should cancel the 5000ms timer and wake
    %% the worker immediately.
    Worker ! {ping, 42},

    %% The worker should reply almost instantly (within ~100ms),
    %% definitely not 5 seconds.
    Result = receive
                 {worker_result, R} -> R
             after 1000 ->
                 exit(timer_not_cancelled)
             end,
    {pong, 42} = Result,

    %% Test 2: Verify that `after 0` fires immediately when no message
    %% is pending.
    Pid2 = spawn(fun() ->
                     Val = receive
                               never_matches -> bad
                           after 0 ->
                               immediate
                           end,
                     Self ! {after_zero, Val}
                 end),

    After0Result = receive
                       {after_zero, V} -> V
                   after 1000 ->
                       exit(after_zero_stuck)
                   end,
    immediate = After0Result,

    %% Test 3: Multiple cancel/wake cycles on the same process (reuse PID slot).
    ok = cancel_loop(5),

    ok.

%% Spawn a worker, let it enter receive after, send it a message,
%% verify it responds. Repeat N times.
cancel_loop(0) -> ok;
cancel_loop(N) ->
    Self = self(),
    W = spawn(fun() ->
                  R = receive
                          go -> got_it
                      after 5000 ->
                          stuck
                      end,
                  Self ! {loop_result, R}
              end),
    receive after 10 -> ok end,
    W ! go,
    got_it = receive {loop_result, V} -> V after 1000 -> exit({cancel_loop_timeout, N}) end,
    cancel_loop(N - 1).
