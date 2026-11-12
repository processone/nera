%% EXPECT: cancel_timer_sync_ok
%% Cowlib cow_http2_machine: terminate must really cancel message timers.
-module(cancel_timer_sync).
-export([test/0]).

test() ->
    R = erlang:start_timer(60000, self(), cancelled),
    ok = erlang:cancel_timer(R, [{async, false}, {info, false}]),
    false = erlang:cancel_timer(R),
    S = erlang:send_after(60000, self(), cancelled_send),
    Remaining = erlang:cancel_timer(S),
    true = is_integer(Remaining) andalso Remaining >= 0 andalso Remaining =< 60000,
    false = erlang:cancel_timer(S, []),
    false = erlang:cancel_timer(make_ref()),
    ok = erlang:cancel_timer(make_ref(), [{info, false}]),
    %% A completed cancellation must not consume an already queued message.
    Done = erlang:start_timer(0, self(), delivered),
    receive {timeout, Done, delivered} = Msg -> self() ! Msg end,
    false = erlang:cancel_timer(Done),
    receive {timeout, Done, delivered} -> ok after 1000 -> error(message_removed) end,
    %% The reference must survive copying to another process and cancellation there.
    Other = erlang:start_timer(60000, self(), cancelled_elsewhere),
    Parent = self(),
    spawn(fun() -> Parent ! {cancelled, erlang:cancel_timer(Other, [{info, false}])} end),
    receive {cancelled, ok} -> ok after 1000 -> error(cancel_not_completed) end,
    false = erlang:cancel_timer(Other),
    %% Unsupported arguments fail rather than claim cancellation.
    lists:foreach(fun(F) -> {error, badarg} = result(F) end, [
        fun() -> erlang:cancel_timer(not_a_reference) end,
        fun() -> erlang:cancel_timer(R, [bad]) end,
        fun() -> erlang:cancel_timer(R, [{async, wrong}]) end,
        fun() -> erlang:cancel_timer(R, [{info, wrong}]) end,
        fun() -> erlang:cancel_timer(R, improper) end,
        fun() -> erlang:cancel_timer(R, [{info, false}|bad]) end
    ]),
    %% Keep an uncancelled timer as a real liveness/delivery control.
    Barrier = erlang:start_timer(20, self(), barrier),
    receive {timeout, Barrier, barrier} -> ok end,
    receive
        {timeout, R, _} -> error(cancelled_timer_delivered);
        cancelled_send -> error(cancelled_send_delivered);
        {timeout, Other, _} -> error(cross_process_cancel_failed)
    after 0 -> ok end,
    io:format("cancel_timer_sync_ok~n").

result(F) -> try F() of X -> {return, X} catch C:E -> {C, E} end.
