%% Tests: send many tagged messages, receive in specific order, verify no loss.
-module(selective_receive_stress).
-export([test/0]).

test() ->
    %% Test 1: Send 500 messages, receive in reverse order
    ok = reverse_order_test(500),

    %% Test 2: Interleaved tags, receive one tag at a time
    ok = interleaved_tags_test(200),

    %% Test 3: Selective receive skipping non-matching messages
    ok = skip_test(),

    io:format("ok~n"),
    ok.

%% Send {msg, 1}..{msg, N}, then receive {msg, N} down to {msg, 1}
reverse_order_test(N) ->
    Self = self(),
    spawn(fun() ->
        send_seq(Self, 1, N)
    end),
    %% Wait for all messages to arrive
    receive after 100 -> ok end,
    recv_reverse(N).

send_seq(_To, I, N) when I > N -> ok;
send_seq(To, I, N) ->
    To ! {msg, I},
    send_seq(To, I + 1, N).

recv_reverse(0) -> ok;
recv_reverse(N) ->
    receive
        {msg, N} -> recv_reverse(N - 1)
    after 2000 ->
        error({missing, N})
    end.

%% Send {a, I} and {b, I} interleaved, receive all {a,_} first then all {b,_}
interleaved_tags_test(N) ->
    Self = self(),
    spawn(fun() ->
        send_interleaved(Self, 1, N)
    end),
    receive after 100 -> ok end,
    ok = recv_tag(a, 1, N),
    ok = recv_tag(b, 1, N),
    ok.

send_interleaved(_To, I, N) when I > N -> ok;
send_interleaved(To, I, N) ->
    To ! {a, I},
    To ! {b, I},
    send_interleaved(To, I + 1, N).

recv_tag(_Tag, I, N) when I > N -> ok;
recv_tag(Tag, I, N) ->
    receive
        {Tag, I} -> recv_tag(Tag, I + 1, N)
    after 2000 ->
        error({missing, Tag, I})
    end.

%% Send 100 noise messages then the target, verify selective receive finds it
skip_test() ->
    Self = self(),
    spawn(fun() ->
        send_noise(Self, 100),
        Self ! {target, 42}
    end),
    receive after 50 -> ok end,
    receive
        {target, 42} -> ok
    after 2000 ->
        error(target_not_found)
    end,
    %% Drain noise
    drain(),
    ok.

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
