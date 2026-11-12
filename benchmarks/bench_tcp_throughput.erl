%% Benchmark: bulk TCP send throughput over loopback.
%%
%% 1 sender pushes ~100 MB (10000 chunks × 10 KB) to 1 receiver.
%% Stresses the send path (partial writes, reactor fallback for
%% WouldBlock) and the recv path (buffered reads).
%%
%% Usage: ./target/release/nera build benchmarks/bench_tcp_throughput.erl -o /tmp/b -e bench_tcp_throughput:start && /tmp/b
-module(bench_tcp_throughput).
-export([start/0]).

start() ->
    {ok, LSock} = gen_tcp:listen(0, [binary, {active, false}]),
    {ok, Port} = inet:port(LSock),
    Chunks = 10000,
    ChunkSize = 10240,  %% 10 KB
    TotalBytes = Chunks * ChunkSize,
    Parent = self(),

    %% Spawn receiver: accept → recv until sender closes → signal parent
    spawn(fun() ->
        {ok, Sock} = gen_tcp:accept(LSock),
        Received = recv_all(Sock, 0),
        gen_tcp:close(Sock),
        Parent ! {receiver_done, Received}
    end),

    %% Sender: connect → send Chunks × ChunkSize → close
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [binary, {active, false}]),
    %% Use binary constructor to avoid large list allocation
    Bits = ChunkSize * 8,
    Chunk = <<0:Bits>>,

    T0 = erlang:monotonic_time(millisecond),
    send_loop(CSock, Chunk, Chunks),
    gen_tcp:close(CSock),
    receive {receiver_done, _Received} -> ok end,
    T1 = erlang:monotonic_time(millisecond),

    gen_tcp:close(LSock),
    io:format("bench_tcp_throughput: ~w MB bulk send~n", [TotalBytes div 1048576]),
    io:format("time_ms:~w~n", [T1 - T0]),
    halt(0).

send_loop(_Sock, _Chunk, 0) -> ok;
send_loop(Sock, Chunk, N) ->
    ok = gen_tcp:send(Sock, Chunk),
    send_loop(Sock, Chunk, N - 1).

recv_all(Sock, Acc) ->
    case gen_tcp:recv(Sock, 0) of
        {ok, Data} ->
            recv_all(Sock, Acc + byte_size(Data));
        {error, _} ->
            Acc
    end.
