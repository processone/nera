%% Benchmark: message-passing throughput for large binaries.
%%
%% Sends a 10KB binary between two processes N times.
%% Large binaries (> 64 bytes) are automatically refc on both BEAM and nera.
%% With refc: each send = Arc clone + 3-word copy (O(1))
%% Without refc: each send = 10KB memcpy (O(n))
%%
%% Usage: time ./target/release/nera run benchmarks/bench_binary_send.erl
-module(bench_binary_send).
-export([start/0]).

start() ->
    BinSize = 10240,  %% 10 KB
    N = 100000,
    Bits = BinSize * 8,
    Bin = <<0:Bits>>,
    BinSize = byte_size(Bin),

    T0 = erlang:monotonic_time(millisecond),

    Self = self(),
    Pid = spawn(fun() -> receiver(Self, N) end),
    sender(Pid, Bin, N),
    receive done -> ok end,

    T1 = erlang:monotonic_time(millisecond),

    io:format("bench_binary_send: ~w messages of ~w bytes~n", [N, BinSize]),
    io:format("time_ms:~w~n", [T1 - T0]),
    ok.

sender(_Pid, _Bin, 0) -> ok;
sender(Pid, Bin, N) ->
    Pid ! {msg, Bin},
    sender(Pid, Bin, N - 1).

receiver(Parent, 0) ->
    Parent ! done;
receiver(Parent, N) ->
    receive
        {msg, _Bin} -> receiver(Parent, N - 1)
    end.
