%% Benchmark: TCP round-trip latency over loopback.
%%
%% 1 server + 1 client, N round-trips of a 1 KB message.
%% Exercises the non-blocking fast path when socket buffers have data,
%% and reactor fallback when they don't.
%%
%% Usage: ./target/release/nera build benchmarks/bench_tcp_ping_pong.erl -o /tmp/b -e bench_tcp_ping_pong:start && /tmp/b
-module(bench_tcp_ping_pong).
-export([start/0]).

start() ->
    {ok, LSock} = gen_tcp:listen(0, [binary, {active, false}]),
    {ok, Port} = inet:port(LSock),
    N = 50000,
    Size = 1024,
    Parent = self(),

    %% Spawn server: accept → loop(recv → send) → signal parent
    spawn(fun() ->
        {ok, SSock} = gen_tcp:accept(LSock),
        server_loop(SSock, N),
        gen_tcp:close(SSock),
        Parent ! server_done
    end),

    %% Client: connect → loop(send → recv) → measure time
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [binary, {active, false}]),
    %% Use binary constructor to avoid large list allocation
    Bits = Size * 8,
    Msg = <<0:Bits>>,

    T0 = erlang:monotonic_time(millisecond),
    client_loop(CSock, Msg, N),
    T1 = erlang:monotonic_time(millisecond),

    gen_tcp:close(CSock),
    receive server_done -> ok end,
    gen_tcp:close(LSock),

    io:format("bench_tcp_ping_pong: ~w round-trips x ~w bytes~n", [N, Size]),
    io:format("time_ms:~w~n", [T1 - T0]),
    halt(0).

client_loop(_Sock, _Msg, 0) -> ok;
client_loop(Sock, Msg, N) ->
    ok = gen_tcp:send(Sock, Msg),
    {ok, _Reply} = gen_tcp:recv(Sock, 0),
    client_loop(Sock, Msg, N - 1).

server_loop(_Sock, 0) -> ok;
server_loop(Sock, N) ->
    {ok, Data} = gen_tcp:recv(Sock, 0),
    ok = gen_tcp:send(Sock, Data),
    server_loop(Sock, N - 1).
