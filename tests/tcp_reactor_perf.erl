-module(tcp_reactor_perf).
-export([test/0]).

%% Performance regression tests for TCP reactor using self-referential
%% ratios (no absolute thresholds). Catches stall patterns where
%% throughput degrades over time.

test() ->
    ok = test_active_once_batch_consistency(),
    ok = test_sustained_throughput_ratio(),
    halt(0).

%% -------------------------------------------------------------------
%% Test 1: Batch consistency — 5 batches of 200 rounds each.
%% If max batch time > 5x median → stall detected.
%% Machine-independent: compares within the same run.
%% -------------------------------------------------------------------
test_active_once_batch_consistency() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    Times = measure_batches(SSock, CSock, 5, 200, []),

    Sorted = lists:sort(Times),
    Median = lists:nth(3, Sorted),
    Max = lists:last(Sorted),

    %% If max > 5x median, something stalled
    case Median > 0 andalso Max > Median * 5 of
        true ->
            erlang:error({batch_variance_stall, Max, Median, Sorted});
        false -> ok
    end,

    ok = gen_tcp:close(CSock),
    ok = gen_tcp:close(SSock),
    ok.

measure_batches(_SSock, _CSock, 0, _Size, Acc) -> lists:reverse(Acc);
measure_batches(SSock, CSock, Batches, Size, Acc) ->
    T0 = erlang:monotonic_time(millisecond),
    ok = perf_rearm_loop(SSock, CSock, Size),
    Elapsed = erlang:monotonic_time(millisecond) - T0,
    measure_batches(SSock, CSock, Batches - 1, Size, [Elapsed | Acc]).

%% -------------------------------------------------------------------
%% Test 2: Sustained throughput ratio — warmup vs sustained.
%% Phase 1 (warmup): 200 rounds. Phase 2 (sustained): 600 rounds.
%% Sustained should take ~3x warmup. If sustained > 9x warmup → stall.
%% Catches "works fine initially then stalls" pattern.
%% -------------------------------------------------------------------
test_sustained_throughput_ratio() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    %% Phase 1: warmup
    T1 = erlang:monotonic_time(millisecond),
    ok = perf_rearm_loop(SSock, CSock, 200),
    Warmup = erlang:monotonic_time(millisecond) - T1,

    %% Phase 2: sustained
    T2 = erlang:monotonic_time(millisecond),
    ok = perf_rearm_loop(SSock, CSock, 600),
    Sustained = erlang:monotonic_time(millisecond) - T2,

    %% Sustained (3x rounds) should take ~3x warmup time.
    %% If it takes >9x warmup, something degraded.
    %% Guard against Warmup=0 (very fast machine) by using max(1, Warmup).
    EffWarmup = max(1, Warmup),
    case Sustained > EffWarmup * 9 of
        true ->
            erlang:error({throughput_regression, Sustained, Warmup});
        false -> ok
    end,

    ok = gen_tcp:close(CSock),
    ok = gen_tcp:close(SSock),
    ok.

%% -------------------------------------------------------------------
%% Shared helper: {active, once} re-arm loop with per-round timeout
%% -------------------------------------------------------------------
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
