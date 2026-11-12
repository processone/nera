%% Tests receive...after timeout functionality.
-module(receive_after).
-export([test/0]).

test() ->
    %% Test 1: receive after 0 with empty mailbox -> immediate timeout
    timeout = receive_after_zero(),

    %% Test 2: receive after 0 with a message waiting
    self() ! hello,
    hello = receive_with_msg(),

    %% Test 3: receive after with short timeout (no message arrives)
    timeout = receive_after_timeout(),

    %% Test 4: receive after 0 in a non-blocking poll (flush pattern)
    self() ! a,
    self() ! b,
    ok = flush(),

    %% Test 5: after body can return a complex value
    {timeout, 42} = receive_after_complex(),

    %% Test 6: receive with only an after clause (no match clauses)
    ok = receive_only_after(),

    ok.

%% Helper: receive with after 0, empty mailbox
receive_after_zero() ->
    receive
        Msg -> Msg
    after 0 ->
        timeout
    end.

%% Helper: receive with after 0, message already in mailbox
receive_with_msg() ->
    receive
        Msg -> Msg
    after 0 ->
        timeout
    end.

%% Helper: receive with a short timeout, no message
receive_after_timeout() ->
    receive
        _Msg -> got_msg
    after 50 ->
        timeout
    end.

%% Flush all messages from the mailbox
flush() ->
    receive
        _Any -> flush()
    after 0 ->
        ok
    end.

%% After body returning a complex value
receive_after_complex() ->
    receive
        Msg -> Msg
    after 0 ->
        {timeout, id(42)}
    end.

%% Receive with only an after clause, no match clauses
receive_only_after() ->
    receive
    after 0 ->
        ok
    end.

id(X) -> X.
