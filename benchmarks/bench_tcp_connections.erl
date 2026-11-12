%% Benchmark: concurrent TCP connection handling.
%%
%% 1 server accepting 100 concurrent clients, each doing a small
%% data exchange (send 64 bytes → recv echo → close).
%% Exercises reactor's ability to manage many simultaneous sockets.
%%
%% Usage: ./target/release/nera build benchmarks/bench_tcp_connections.erl -o /tmp/b -e bench_tcp_connections:start && /tmp/b
-module(bench_tcp_connections).
-export([start/0]).

start() ->
    {ok, LSock} = gen_tcp:listen(0, [binary, {active, false}, {backlog, 256}]),
    {ok, Port} = inet:port(LSock),
    NumClients = 100,
    Parent = self(),

    %% Spawn acceptor: accepts NumClients connections, each handler
    %% echoes one message then closes.
    spawn(fun() -> accept_loop(LSock, NumClients) end),

    T0 = erlang:monotonic_time(millisecond),

    %% Spawn NumClients client processes in parallel
    lists:foreach(fun(_) ->
        spawn(fun() ->
            {ok, Sock} = gen_tcp:connect({127,0,0,1}, Port, [binary, {active, false}]),
            Msg = <<"hello from client, this is a 64-byte payload for benchmark!!!!">>,
            ok = gen_tcp:send(Sock, Msg),
            _ = gen_tcp:recv(Sock, 0),
            gen_tcp:close(Sock),
            Parent ! client_done
        end)
    end, lists:seq(1, NumClients)),

    wait_clients(NumClients),

    T1 = erlang:monotonic_time(millisecond),

    gen_tcp:close(LSock),
    io:format("bench_tcp_connections: ~w concurrent connections~n", [NumClients]),
    io:format("time_ms:~w~n", [T1 - T0]),
    halt(0).

accept_loop(_LSock, 0) -> ok;
accept_loop(LSock, N) ->
    {ok, Sock} = gen_tcp:accept(LSock),
    spawn(fun() ->
        case gen_tcp:recv(Sock, 0) of
            {ok, Data} ->
                _ = gen_tcp:send(Sock, Data),
                gen_tcp:close(Sock);
            {error, _} ->
                gen_tcp:close(Sock)
        end
    end),
    accept_loop(LSock, N - 1).

wait_clients(0) -> ok;
wait_clients(N) ->
    receive client_done -> ok end,
    wait_clients(N - 1).
