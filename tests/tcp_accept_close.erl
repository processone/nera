-module(tcp_accept_close).
-export([test/0]).
%% EXPECT: accept_close_ok
test() ->
    ok = pending_close(1),
    ok = pending_close(16),
    ok = close_races(20),
    io:format("accept_close_ok~n"),
    halt(0).

pending_close(Count) ->
    {ok, Listener} = gen_tcp:listen(0, [{active, false}, binary]),
    Parent = self(),
    Pids = [spawn_link(fun() -> Parent ! {accepted, self(), gen_tcp:accept(Listener)} end)
            || _ <- lists:seq(1, Count)],
    lists:foreach(fun(Pid) -> await_waiting(Pid, 5000) end, Pids),
    ok = gen_tcp:close(Listener),
    await_results(Pids).

close_races(0) -> ok;
close_races(N) ->
    {ok, Listener} = gen_tcp:listen(0, [{active, false}, binary]),
    Parent = self(),
    Pids = [spawn_link(fun() -> Parent ! {accepted, self(), gen_tcp:accept(Listener)} end)
            || _ <- lists:seq(1, 4)],
    ok = gen_tcp:close(Listener),
    ok = await_results(Pids),
    close_races(N - 1).

await_waiting(_Pid, 0) -> fail(accept_not_waiting);
await_waiting(Pid, N) ->
    case process_info(Pid, status) of
        {status, waiting} -> ok;
        _ -> receive after 1 -> ok end, await_waiting(Pid, N - 1)
    end.

await_results([]) -> ok;
await_results([Pid | Rest]) ->
    receive
        {accepted, Pid, {error, closed}} -> await_results(Rest);
        {accepted, Pid, Other} -> fail({unexpected_accept_result, Other})
    after 5000 -> fail({accept_close_timeout, Pid})
    end.

fail(Reason) ->
    io:format("accept_close_failed: ~p~n", [Reason]),
    halt(1).
