%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (Ch. 12 flush)
%% Tests: flush mailbox with receive _ -> ... after 0 -> ok end, after 0 semantics
-module(jaerlang2_flush).
-export([test/0]).

test() ->
    %% Test 1: Flush empties the mailbox
    self() ! a,
    self() ! b,
    self() ! c,
    flush(),
    %% Mailbox should be empty - after 0 fires immediately
    empty = receive _ -> not_empty after 0 -> empty end,

    %% Test 2: Flush on empty mailbox is a no-op
    flush(),
    empty = receive _ -> not_empty after 0 -> empty end,

    %% Test 3: after 0 returns immediately when no matching messages
    self() ! {specific, tag},
    %% No {other, _} messages, so after 0 fires
    no_match = receive {other, _} -> matched after 0 -> no_match end,
    %% The {specific, tag} message is still in the mailbox
    {specific, tag} = receive {specific, T} -> {specific, T} end,

    %% Test 4: Flush with count
    self() ! x,
    self() ! y,
    self() ! z,
    3 = flush_count(0),
    empty = receive _ -> not_empty after 0 -> empty end,

    %% Test 5: Selective flush - discard only matching messages
    self() ! {keep, 1},
    self() ! {discard, 2},
    self() ! {keep, 3},
    self() ! {discard, 4},
    selective_flush(),
    %% Only {keep, _} messages should remain
    1 = receive {keep, K1} -> K1 end,
    3 = receive {keep, K2} -> K2 end,
    empty = receive _ -> not_empty after 0 -> empty end,

    ok.

%% Flush all messages from the mailbox
flush() ->
    receive
        _ -> flush()
    after 0 ->
        ok
    end.

%% Flush and count how many messages were drained
flush_count(N) ->
    receive
        _ -> flush_count(N + 1)
    after 0 ->
        N
    end.

%% Selectively flush only {discard, _} messages
selective_flush() ->
    receive
        {discard, _} -> selective_flush()
    after 0 ->
        ok
    end.
