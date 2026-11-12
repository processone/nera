%% Stress test: tombstone-heavy mailbox patterns.
%% Exercises the tombstone + compaction mailbox under high selective receive load.
-module(selective_receive_tombstone_stress).
-export([test/0]).

test() ->
    ok = alternating_accept_test(500),
    ok = drain_after_selective_test(1000),
    ok = deep_skip_test(200),
    io:format("ok~n"),
    ok.

%% Send 2*N messages with alternating tags (even/odd).
%% Selectively receive all even-tagged messages first (creating N tombstones),
%% then receive all odd-tagged messages and verify order.
alternating_accept_test(N) ->
    Self = self(),
    spawn(fun() -> send_alternating(Self, 0, 2 * N) end),
    receive after 100 -> ok end,
    %% Receive all even-tagged messages: {even, 0}, {even, 2}, ...
    ok = recv_even(0, N),
    %% Now receive all odd-tagged messages: {odd, 1}, {odd, 3}, ...
    ok = recv_odd(1, N),
    ok.

send_alternating(_To, I, Max) when I >= Max -> ok;
send_alternating(To, I, Max) ->
    case I rem 2 of
        0 -> To ! {even, I};
        1 -> To ! {odd, I}
    end,
    send_alternating(To, I + 1, Max).

recv_even(I, N) when I >= 2 * N -> ok;
recv_even(I, N) ->
    receive
        {even, I} -> recv_even(I + 2, N)
    after 2000 ->
        error({missing_even, I})
    end.

recv_odd(I, N) when I >= 2 * N -> ok;
recv_odd(I, N) ->
    receive
        {odd, I} -> recv_odd(I + 2, N)
    after 2000 ->
        error({missing_odd, I})
    end.

%% Send N messages, selectively receive just one from the middle,
%% then drain the rest and verify count.
drain_after_selective_test(N) ->
    Self = self(),
    Target = N div 2,
    spawn(fun() -> send_seq(Self, 1, N) end),
    receive after 100 -> ok end,
    %% Selectively receive the target from the middle
    receive
        {msg, Target} -> ok
    after 2000 ->
        error({missing_target, Target})
    end,
    %% Drain remaining — should be N-1 messages
    Count = drain_count(0),
    Expected = N - 1,
    case Count of
        Expected -> ok;
        _ -> error({wrong_drain_count, Count, Expected})
    end.

send_seq(_To, I, N) when I > N -> ok;
send_seq(To, I, N) ->
    To ! {msg, I},
    send_seq(To, I + 1, N).

drain_count(Acc) ->
    receive
        _ -> drain_count(Acc + 1)
    after 0 ->
        Acc
    end.

%% Send N noise messages then a target. Skip all noise via pattern match.
%% Repeat multiple times to exercise tombstone compaction.
deep_skip_test(N) ->
    deep_skip_loop(N, 5).

deep_skip_loop(_N, 0) -> ok;
deep_skip_loop(N, Rounds) ->
    Self = self(),
    spawn(fun() ->
        send_noise(Self, N),
        Self ! {target, Rounds}
    end),
    receive after 50 -> ok end,
    receive
        {target, Rounds} -> ok
    after 2000 ->
        error({missing_target, Rounds})
    end,
    %% Drain noise
    drain(),
    deep_skip_loop(N, Rounds - 1).

send_noise(_To, 0) -> ok;
send_noise(To, N) ->
    To ! {noise, N},
    send_noise(To, N - 1).

drain() ->
    receive
        _ -> drain()
    after 0 ->
        ok
    end.
