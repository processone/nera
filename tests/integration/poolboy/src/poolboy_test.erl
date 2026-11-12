%% Integration test for the poolboy worker pool library compiled with Nera.
%% Exercises checkout/checkin, transactions, and overflow workers.
%%
%% poolboy:start_link/2 returns {ok, Pid}.
%% poolboy:checkout/1,2,3 returns a worker Pid (or full).
%% poolboy:checkin/2 returns ok.
%% poolboy:transaction/2 takes a fun and returns its result.
%% poolboy:status/1 returns {StateName, Available, Overflow, Monitors}.
-module(poolboy_test).
-export([test/0]).

test() ->
    %% ── Start a pool with 2 workers + 1 overflow slot ──
    PoolArgs =
        [{name, {local, my_pool}}, {worker_module, poolboy_worker}, {size, 2}, {max_overflow, 1}],
    WorkerArgs = [],
    {ok, _Pid} = poolboy:start_link(PoolArgs, WorkerArgs),
    io:format("pool started: ok~n"),

    %% ── Basic checkout / call / checkin ──
    Worker = poolboy:checkout(my_pool),
    pong = gen_server:call(Worker, ping),
    ok = poolboy:checkin(my_pool, Worker),
    io:format("checkout/checkin: ok~n"),

    %% ── Transaction helper ──
    hello = poolboy:transaction(my_pool, fun (W) -> gen_server:call(W, {echo, hello})
        end),
    io:format("transaction: ok~n"),

    %% ── Saturate the pool, then exercise overflow ──
    W1 = poolboy:checkout(my_pool),
    W2 = poolboy:checkout(my_pool),
    W3 = poolboy:checkout(my_pool, true, 1000),
    pong = gen_server:call(W1, ping),
    pong = gen_server:call(W2, ping),
    pong = gen_server:call(W3, ping),
    ok = poolboy:checkin(my_pool, W1),
    ok = poolboy:checkin(my_pool, W2),
    ok = poolboy:checkin(my_pool, W3),
    io:format("overflow: ok~n"),

    %% ── Status reporting ──
    {_StateName, _Available, _Overflow, _Monitors} = poolboy:status(my_pool),
    io:format("status: ok~n"),

    %% ── Stop the pool cleanly ──
    ok = poolboy:stop(my_pool),
    io:format("stop: ok~n"),

    io:format("all tests passed~n"),
    ok.
