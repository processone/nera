%% Benchmark: selective receive throughput with deep mailboxes.
%%
%% Exercises the O(1) tombstone removal vs O(n) shift on each accepted message.
%%
%% Test 1: "Deep skip" — mailbox has N noise messages, then the target.
%%   Selective receive skips all noise to find the target.
%%   Repeat R rounds, measuring total time.
%%
%% Test 2: "Reverse order" — send N tagged messages, receive them in reverse.
%%   Each accept creates a tombstone deep in the mailbox.
%%   Old O(n) shift: O(N^2) total.  New tombstone: O(N) total.
%%
%% Usage: time ./target/release/nera run benchmarks/bench_selective_receive.erl
-module(bench_selective_receive).
-export([start/0]).

start() ->
    ok = bench_deep_skip(500, 200),
    ok = bench_reverse_order(5000),
    ok.

%% Send Depth noise messages then a target. Selectively receive the target,
%% then drain the noise. Repeat Rounds times.
bench_deep_skip(Depth, Rounds) ->
    T0 = erlang:monotonic_time(millisecond),
    ok = deep_skip_loop(Depth, Rounds),
    T1 = erlang:monotonic_time(millisecond),
    io:format("bench_deep_skip(depth=~w, rounds=~w): time_ms:~w~n",
              [Depth, Rounds, T1 - T0]),
    ok.

deep_skip_loop(_Depth, 0) -> ok;
deep_skip_loop(Depth, N) ->
    Self = self(),
    spawn(fun() ->
        send_noise(Self, Depth),
        Self ! {target, N}
    end),
    %% Let messages arrive
    receive after 10 -> ok end,
    %% Selective receive: skip noise, find target
    receive
        {target, N} -> ok
    after 5000 ->
        error({missing_target, N})
    end,
    %% Drain noise
    drain(),
    deep_skip_loop(Depth, N - 1).

%% Send {msg, 1}..{msg, N}, then receive in reverse order {msg, N}..{msg, 1}.
%% Each receive must scan deeper into the mailbox, creating a tombstone.
bench_reverse_order(N) ->
    T0 = erlang:monotonic_time(millisecond),
    Self = self(),
    spawn(fun() -> send_seq(Self, 1, N) end),
    receive after 100 -> ok end,
    ok = recv_reverse(N),
    T1 = erlang:monotonic_time(millisecond),
    io:format("bench_reverse_order(n=~w): time_ms:~w~n", [N, T1 - T0]),
    ok.

send_seq(_To, I, N) when I > N -> ok;
send_seq(To, I, N) ->
    To ! {msg, I},
    send_seq(To, I + 1, N).

recv_reverse(0) -> ok;
recv_reverse(N) ->
    receive
        {msg, N} -> recv_reverse(N - 1)
    after 5000 ->
        error({missing, N})
    end.

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
