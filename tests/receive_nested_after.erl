%% EXPECT: receive_nested_after_ok
%% A receive directly inside an after body must resume itself, not its
%% enclosing receive, and preserve values evaluated before the inner wait.
%% Run this fixture unchanged natively and on BEAM.
-module(receive_nested_after).
-export([test/0]).

-define(WAIT_MS, 120).

test() ->
    {aft, {late, x}} = delayed_message(),
    io:format("delayed message and earlier atom ok~n"),
    {{kept, [1, 2], <<3, 4>>}, nolate} = own_timeout(),
    io:format("inner timeout and earlier value ok~n"),
    {tail, x} = tail_receive(),
    io:format("tail receive without timeout ok~n"),
    {outer, {middle, {late, x}}} = three_levels(),
    io:format("three nested receives ok~n"),
    {first, x, second, y} = successive_receives(),
    io:format("successive inner receives ok~n"),
    io:format("receive_nested_after_ok~n"),
    ok.

delayed_message() ->
    Sender = sender(self(), x),
    receive never -> unexpected after 10 ->
        %% Start the sender only once the enclosing receive has timed out.
        Sender ! go,
        {aft, receive X -> {late, X} after 1000 -> nolate end}
    end.

own_timeout() ->
    receive never -> unexpected after 10 ->
        Started = erlang:monotonic_time(millisecond),
        Keep = {kept, [1, 2], <<3, 4>>},
        R = {Keep, receive never -> unexpected after ?WAIT_MS -> nolate end},
        Elapsed = erlang:monotonic_time(millisecond) - Started,
        %% Allow clock quantization, not an early wake on the outer deadline.
        true = Elapsed >= ?WAIT_MS - 2,
        R
    end.

tail_receive() ->
    Sender = sender(self(), x),
    receive never -> unexpected after 0 ->
        Sender ! go,
        receive X -> {tail, X} end
    end.

three_levels() ->
    Sender = sender(self(), x),
    receive never -> unexpected after 10 ->
        {outer, receive never -> unexpected after 10 ->
            Sender ! go,
            {middle, receive X -> {late, X} after 1000 -> nolate end}
        end}
    end.

successive_receives() ->
    Sender1 = sender(self(), x),
    Sender2 = sender(self(), y),
    receive never -> unexpected after 0 ->
        Sender1 ! go,
        A = {first, receive X -> X after 1000 -> nolate end},
        Sender2 ! go,
        {first, V} = A,
        {first, V, second, receive Y -> Y after 1000 -> nolate end}
    end.

sender(To, Value) ->
    spawn(fun() ->
        receive go ->
            receive after 30 -> To ! Value end
        end
    end).
