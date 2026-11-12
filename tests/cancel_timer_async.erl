%% EXPECT: cancel_timer_async_ok
%% Cowlib's frame handlers also request asynchronous cancellation without info.
-module(cancel_timer_async).
-export([test/0]).

test() ->
    R = erlang:start_timer(60000, self(), cancelled),
    ok = erlang:cancel_timer(R, [{async, true}, {info, false}]),
    S = erlang:send_after(60000, self(), cancelled_send),
    Cancel = fun erlang:cancel_timer/2,
    ok = Cancel(S, [{async, true}]),
    receive
        {cancel_timer, S, Remaining} ->
            true = is_integer(Remaining) andalso Remaining >= 0 andalso Remaining =< 60000
    after 1000 -> error(no_cancel_reply) end,
    Unknown = make_ref(),
    ok = apply(erlang, cancel_timer, [Unknown, [{async, true}]]),
    receive {cancel_timer, Unknown, false} -> ok after 1000 -> error(no_false_reply) end,
    Barrier = erlang:start_timer(20, self(), barrier),
    receive {timeout, Barrier, barrier} -> ok end,
    receive
        {cancel_timer, R, _} -> error(unrequested_info);
        {timeout, R, _} -> error(cancelled_timer_delivered);
        cancelled_send -> error(cancelled_send_delivered)
    after 0 -> ok end,
    io:format("cancel_timer_async_ok~n").
