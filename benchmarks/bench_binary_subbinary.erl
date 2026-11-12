%% Benchmark: sub-binary extraction throughput.
%%
%% Extracts rest sub-binaries from a 100KB source N times.
%% Large binaries (> 64 bytes) are automatically refc on both BEAM and nera.
%% With refc: sub-binary shares Arc (O(1) for large result)
%% Without refc: each extraction copies bytes (O(n))
%%
%% Usage: time ./target/release/nera run benchmarks/bench_binary_subbinary.erl
-module(bench_binary_subbinary).
-export([start/0]).

start() ->
    BinSize = 102400,  %% 100 KB
    N = 100000,
    Bits = BinSize * 8,
    Bin = <<0:Bits>>,
    BinSize = byte_size(Bin),

    T0 = erlang:monotonic_time(millisecond),

    sub_loop(Bin, N),

    T1 = erlang:monotonic_time(millisecond),

    io:format("bench_binary_subbinary: ~w sub-binary extractions from ~w bytes~n", [N, BinSize]),
    io:format("time_ms:~w~n", [T1 - T0]),
    ok.

sub_loop(_Bin, 0) -> ok;
sub_loop(Bin, N) ->
    %% Extract rest after first byte (result is BinSize-1 bytes)
    <<_, Rest/binary>> = Bin,
    %% Use Rest so the compiler doesn't optimize it away
    _ = byte_size(Rest),
    sub_loop(Bin, N - 1).
