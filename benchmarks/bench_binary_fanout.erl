%% Benchmark: memory efficiency when many processes share one binary.
%%
%% Creates a 100KB binary, sends to N processes.
%% Large binaries (> 64 bytes) are automatically refc on both BEAM and nera.
%% With refc: total memory ~ 100KB + N*24 bytes (descriptors)
%% Without refc: total memory ~ N*100KB
%%
%% Measure peak RSS with: /usr/bin/time -l ./bench_binary_fanout
%%
%% Usage: time ./target/release/nera run benchmarks/bench_binary_fanout.erl
-module(bench_binary_fanout).
-export([start/0]).

start() ->
    BinSize = 102400,  %% 100 KB
    N = 1000,
    Bits = BinSize * 8,
    Bin = <<0:Bits>>,
    BinSize = byte_size(Bin),

    T0 = erlang:monotonic_time(millisecond),

    Self = self(),
    Pids = spawn_holders(N, Bin, Self),

    %% Wait for all holders to confirm they have the binary
    wait_ready(N),

    T1 = erlang:monotonic_time(millisecond),

    io:format("bench_binary_fanout: ~w processes each holding ~w bytes~n", [N, BinSize]),
    io:format("time_ms:~w~n", [T1 - T0]),

    %% Tell them to exit
    signal_done(Pids),
    wait_done(N),
    ok.

spawn_holders(0, _Bin, _Parent) -> [];
spawn_holders(N, Bin, Parent) ->
    %% Send binary via message so deep_copy creates refc Arc clone
    Pid = spawn(fun() -> holder_init(Parent) end),
    Pid ! {binary, Bin},
    [Pid | spawn_holders(N - 1, Bin, Parent)].

holder_init(Parent) ->
    receive
        {binary, Bin} ->
            _ = byte_size(Bin),
            Parent ! ready,
            receive
                done ->
                    _ = byte_size(Bin),
                    Parent ! exited
            end
    end.

signal_done([]) -> ok;
signal_done([Pid | Rest]) ->
    Pid ! done,
    signal_done(Rest).

wait_ready(0) -> ok;
wait_ready(N) ->
    receive ready -> wait_ready(N - 1) end.

wait_done(0) -> ok;
wait_done(N) ->
    receive exited -> wait_done(N - 1) end.
