%% EXPECT: many_pending_timers_ok
%% Tens of thousands of pending message timers must fit in one program.
%% Each start_timer/send_after used to keep an OS thread until it fired or
%% was cancelled; on macOS, about 4,000 pending timers aborted the program.
-module(many_pending_timers).
-export([test/0]).

-define(COUNT, 25000).

test() ->
    Self = self(),
    Timers = [erlang:start_timer(60000, Self, N) || N <- lists:seq(1, ?COUNT)],
    Sends = [erlang:send_after(60000, Self, {late, N}) || N <- lists:seq(1, ?COUNT)],
    %% Short timers still fire, earliest first, among the pending ones.
    Short = [erlang:start_timer(Ms, Self, Ms) || Ms <- [30, 10, 20]],
    [10, 20, 30] = [receive {timeout, R, Ms} -> true = lists:member(R, Short), Ms end
                    || _ <- Short],
    Left = [erlang:cancel_timer(R) || R <- Timers ++ Sends],
    true = lists:all(fun(Ms) -> is_integer(Ms) andalso Ms > 0 andalso Ms =< 60000 end, Left),
    receive Unexpected -> error({unexpected, Unexpected}) after 0 -> ok end,
    io:format("many_pending_timers_ok~n").
