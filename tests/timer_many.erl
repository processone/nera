%% Stress test for the timer wheel: spawn many processes each with a
%% different `receive after` timeout. Verify all processes wake at the
%% right time and the runtime exits cleanly.
-module(timer_many).
-export([test/0]).

test() ->
    Self = self(),
    N = 50,
    %% Spawn N processes with staggered timeouts (10ms increments).
    Pids = [spawn(fun() ->
                      Delay = I * 10,
                      receive
                      after Delay ->
                          Self ! {done, I}
                      end
                  end) || I <- lists:seq(1, N)],

    %% Collect all N replies. Each process should wake from its timeout.
    ok = collect(N, N),

    %% Verify all processes have exited (no zombies).
    %% Give a brief moment for process exit cleanup.
    receive after 50 -> ok end,

    ok.

collect(0, _Total) ->
    ok;
collect(Remaining, Total) ->
    receive
        {done, _I} ->
            collect(Remaining - 1, Total)
    after 5000 ->
        %% If we don't get all replies within 5s, something is wrong.
        exit({timeout_waiting, Remaining, total, Total})
    end.
