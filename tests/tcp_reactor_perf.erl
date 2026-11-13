-module(tcp_reactor_perf).
-export([test/0]).

%% Performance regression tests for the TCP reactor's {active, once} re-arm
%% path, using self-referential ratios (no absolute thresholds). They catch
%% stall patterns: re-arms that keep waiting for a fallback wakeup, and
%% throughput that degrades while a connection keeps running.
%%
%% One slow batch is not a pattern. On a busy host, a burst of activity
%% slows many rounds a little, and the host can pause the whole program for
%% tens of milliseconds. Either can put one batch nearly 100 ms over the
%% others while no single round takes that long. On a loaded host an
%% equivalent loop on OTP 29 also fails a single-batch rule. Each check
%% therefore fails only when the slowdown shows in at least two batches.
%% Batches are timed in microseconds.

test() ->
    ok = test_active_once_batch_consistency(),
    ok = test_sustained_throughput_ratio(),
    halt(0).

%% -------------------------------------------------------------------
%% Test 1: Batch consistency — 10 batches of 200 rounds, alternating
%% between two connections. A batch over 5x the median batch is stalled;
%% two stalled batches are a stall pattern. That covers a stall that
%% recurs within a connection and one that hits every connection (each
%% connection's first batch holds its first re-arm).
%% -------------------------------------------------------------------
test_active_once_batch_consistency() ->
    A = connected_pair(),
    B = connected_pair(),
    Times = measure_batches(A, B, 10, 200, []),

    Median = lists:nth(5, lists:sort(Times)),
    Stalled = [T || T <- Times, T > Median * 5],
    case length(Stalled) >= 2 of
        true ->
            erlang:error({batch_variance_stall, Stalled, Median, Times});
        false -> ok
    end,

    ok = close_pair(A),
    ok = close_pair(B),
    ok.

measure_batches(_Pair, _Other, 0, _Size, Acc) -> lists:reverse(Acc);
measure_batches(Pair, Other, Batches, Size, Acc) ->
    Elapsed = timed_rounds(Pair, Size),
    measure_batches(Other, Pair, Batches - 1, Size, [Elapsed | Acc]).

%% -------------------------------------------------------------------
%% Test 2: Sustained throughput — on a fresh connection, 200 warmup
%% rounds, then 600 sustained rounds in three batches of 200. Each
%% sustained batch should take about as long as the warmup; two over 3x
%% the warmup are a degradation. Catches the "works fine initially then
%% stalls" pattern.
%% -------------------------------------------------------------------
test_sustained_throughput_ratio() ->
    Pair = connected_pair(),

    Warmup = max(1, timed_rounds(Pair, 200)),
    Sustained = [timed_rounds(Pair, 200) || _ <- [1, 2, 3]],

    Degraded = [T || T <- Sustained, T > Warmup * 3],
    case length(Degraded) >= 2 of
        true ->
            erlang:error({throughput_regression, Sustained, Warmup});
        false -> ok
    end,

    ok = close_pair(Pair),
    ok.

%% -------------------------------------------------------------------
%% Shared helpers
%% -------------------------------------------------------------------
connected_pair() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),
    {SSock, CSock}.

close_pair({SSock, CSock}) ->
    ok = gen_tcp:close(CSock),
    gen_tcp:close(SSock).

%% Microseconds taken by N re-arm rounds.
timed_rounds({SSock, CSock}, N) ->
    T0 = erlang:monotonic_time(microsecond),
    ok = perf_rearm_loop(SSock, CSock, N),
    erlang:monotonic_time(microsecond) - T0.

%% {active, once} re-arm loop with per-round timeout.
perf_rearm_loop(_SSock, _CSock, 0) -> ok;
perf_rearm_loop(SSock, CSock, N) ->
    ok = inet:setopts(SSock, [{active, once}]),
    ok = gen_tcp:send(CSock, <<"P\r\n">>),
    receive
        {tcp, SSock, <<"P">>} -> ok
    after 2000 ->
        erlang:error({timeout_perf_rearm, N})
    end,
    perf_rearm_loop(SSock, CSock, N - 1).
