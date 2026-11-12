-module(tcp_accept_concurrent).
-export([test/0]).

%% EXPECT: concurrent_accept_ok
%% All acceptors must survive sharing one listening socket. Previously the
%% reactor replaced an earlier pending accept with the last registration.
test() ->
    {ok, Listen} = gen_tcp:listen(0, [{active, false}, binary]),
    {ok, Port} = inet:port(Listen),
    ok = rounds(Listen, Port, 20),
    ok = gen_tcp:close(Listen),
    io:format("concurrent_accept_ok~n"),
    halt(0).

rounds(_Listen, _Port, 0) -> ok;
rounds(Listen, Port, N) ->
    Parent = self(),
    Pids = [spawn_link(fun() ->
        {ok, Socket} = gen_tcp:accept(Listen),
        ok = gen_tcp:close(Socket),
        Parent ! {accepted, self()}
    end) || _ <- lists:seq(1, 3)],
    %% Do not connect until every acceptor is suspended inside accept/1.
    %% The Rust reactor test additionally forces registration order directly.
    ok = await_acceptors(Pids),
    Clients = [begin
        {ok, Socket} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
        Socket
    end || _ <- Pids],
    ok = await_results(Pids),
    lists:foreach(fun(Socket) -> ok = gen_tcp:close(Socket) end, Clients),
    rounds(Listen, Port, N - 1).

await_acceptors([]) -> ok;
await_acceptors([Pid | Rest]) ->
    ok = await_waiting(Pid, 5000),
    await_acceptors(Rest).

await_waiting(Pid, 0) ->
    io:format("acceptor_not_waiting: ~p~n", [Pid]),
    halt(1);
await_waiting(Pid, Attempts) ->
    case process_info(Pid, status) of
        {status, waiting} -> ok;
        _ ->
            receive after 1 -> ok end,
            await_waiting(Pid, Attempts - 1)
    end.

await_results([]) -> ok;
await_results([Pid | Rest]) ->
    receive
        {accepted, Pid} -> await_results(Rest)
    after 5000 ->
        io:format("acceptor_lost: ~p~n", [Pid]),
        halt(1)
    end.
