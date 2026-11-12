%% EXPECT: receive_timeout_domain_ok
%% A receive's after value must be infinity or an integer in 0..4294967295.
%% Any other value raises error:timeout_value, but only when the receive would
%% wait for a message, as on BEAM: a matching message already in the mailbox
%% is received first. Messages that do not match stay queued, and an invalid
%% timeout never waits for a message sent later. Run this same fixture on
%% native Nera and BEAM.
-module(receive_timeout_domain).
-export([test/0]).

test() ->
    lists:foreach(fun bad/1, [-1, invalid, [], 1.5, 4294967296, 1152921504606846976]),

    %% The valid boundaries.
    self() ! ready,
    ready = finite(4294967295),
    elapsed = finite(0),
    Parent = self(),
    spawn(fun() -> timer:sleep(10), Parent ! later end),
    later = finite(1000),

    %% With an empty mailbox, a protected timeout failure skips the rest of
    %% the try body.
    caught = try receive _ -> consumed after invalid -> elapsed end,
                 error(continued_after_invalid)
             catch error:timeout_value -> caught end,

    %% Inside try, a matching message is received and the body continues.
    self() ! inside,
    {continued, inside} =
        try {continued, receive M -> M after id(invalid) -> elapsed end}
        catch error:timeout_value -> raised end,

    %% The after expression runs before the mailbox scan. Its own error, or
    %% an earlier one in the try body, is raised first: the queued message
    %% stays, and the error is not replaced by timeout_value.
    self() ! go,
    caught = try _ = 1 + id(a), receive go -> consumed after 10 -> elapsed end
             catch error:badarith -> caught end,
    caught = try receive go -> consumed after (1 div id(0)) -> elapsed end
             catch error:badarith -> caught end,
    {'EXIT', {badarith, _}} =
        (catch receive go -> consumed after (1 div id(0)) -> elapsed end),
    go = next(),
    caught = try receive _ -> consumed after (1 div id(0)) -> elapsed end
             catch error:badarith -> caught end,

    %% A receive with only an after clause has nothing to match: it raises
    %% even with a message queued, and the message stays.
    self() ! queued,
    {'EXIT', {timeout_value, _}} = (catch receive after id(-1) -> elapsed end),
    queued = next(),

    %% An invalid timeout does not wait for a message sent later.
    spawn(fun() -> timer:sleep(10), Parent ! late end),
    {'EXIT', {timeout_value, _}} =
        (catch receive late -> early after id(invalid) -> elapsed end),
    late = finite(1000),

    %% A message received with an invalid timeout leaves nothing behind: the
    %% next receives time out and wait as usual.
    self() ! one,
    one = receive one -> one after id(invalid) -> elapsed end,
    elapsed = receive never -> never after 10 -> elapsed end,
    spawn(fun() -> timer:sleep(10), Parent ! two end),
    two = receive two -> two end,

    %% An uncaught failure ends the process with reason timeout_value. BEAM
    %% adds the stack to the reason; native Nera gives the bare reason.
    {Pid, Ref} = spawn_monitor(fun() -> finite(invalid) end),
    receive {'DOWN', Ref, process, Pid, timeout_value} -> ok;
            {'DOWN', Ref, process, Pid, {timeout_value, _}} -> ok
    after 2000 -> error(uncaught_timeout_error_lost) end,

    %% A spawned process receives its queued message and exits normally.
    {Pid2, Ref2} = spawn_monitor(fun() -> self() ! go, go = finite(invalid) end),
    receive {'DOWN', Ref2, process, Pid2, normal} -> ok
    after 2000 -> error(queued_message_not_received) end,

    missing = next(),
    io:format("receive_timeout_domain_ok~n"),
    ok.

bad(T) ->
    %% A matching message already in the mailbox is received.
    self() ! held,
    consumed = receive held -> consumed after T -> elapsed end,

    %% Selective receive: the match behind another message is received, and
    %% the other message stays.
    self() ! other, self() ! held,
    consumed = receive held -> consumed after T -> elapsed end,

    %% Nothing matches: timeout_value, and the queued message stays.
    {'EXIT', {timeout_value, _}} =
        (catch receive held -> consumed after T -> elapsed end),
    other = next(),

    %% A guard rejects the only message: timeout_value, and it stays.
    self() ! 1,
    {'EXIT', {timeout_value, _}} =
        (catch receive N when N > 5 -> big after T -> elapsed end),
    1 = next(),

    %% An empty mailbox: timeout_value.
    {'EXIT', {timeout_value, _}} =
        (catch receive _ -> consumed after T -> elapsed end),
    missing = next().

finite(T) -> receive M -> M after T -> elapsed end.

next() -> receive M -> M after 0 -> missing end.

id(X) -> X.
