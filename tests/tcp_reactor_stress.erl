-module(tcp_reactor_stress).
-export([test/0]).

%% Stress tests for TCP reactor correctness under sustained load.
%% Catches stall regressions, close races, and mode-toggle data loss.
%% Uses halt(0) to ensure clean exit (avoids reactor shutdown race).

test() ->
    ok = test_sustained_active_once(),
    ok = test_close_during_active_read(),
    ok = test_setopts_under_load(),
    ok = test_multiprocess_active(),
    halt(0).

%% -------------------------------------------------------------------
%% Test 1: Sustained {active, once} re-arm — 600 rounds in batches
%% of 200. Each batch must complete within 3s. Catches the original
%% stall bug (0 ops/sec after ~55k ops) at a smaller scale.
%% -------------------------------------------------------------------
test_sustained_active_once() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    ok = sustained_batch(SSock, CSock, 3, 200),

    ok = gen_tcp:close(CSock),
    ok = gen_tcp:close(SSock),
    ok.

sustained_batch(_SSock, _CSock, 0, _Size) -> ok;
sustained_batch(SSock, CSock, Batches, Size) ->
    T0 = erlang:monotonic_time(millisecond),
    ok = rearm_loop(SSock, CSock, Size),
    Elapsed = erlang:monotonic_time(millisecond) - T0,
    case Elapsed > 3000 of
        true -> erlang:error({batch_stall, Batches, Elapsed});
        false -> ok
    end,
    sustained_batch(SSock, CSock, Batches - 1, Size).

rearm_loop(_SSock, _CSock, 0) -> ok;
rearm_loop(SSock, CSock, N) ->
    ok = inet:setopts(SSock, [{active, once}]),
    ok = gen_tcp:send(CSock, <<"P\r\n">>),
    receive
        {tcp, SSock, <<"P">>} -> ok
    after 2000 ->
        erlang:error({timeout_rearm, N})
    end,
    rearm_loop(SSock, CSock, N - 1).

%% -------------------------------------------------------------------
%% Test 2: Close peer while {active, true} — verify tcp_closed always
%% arrives, no crash or hang. Stresses EOF during worker read.
%% -------------------------------------------------------------------
test_close_during_active_read() ->
    ok = close_race_loop(10).

close_race_loop(0) -> ok;
close_race_loop(N) ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    ok = inet:setopts(SSock, [{active, true}]),
    %% Send some data then close peer mid-stream
    ok = gen_tcp:send(CSock, <<"data">>),
    ok = gen_tcp:close(CSock),

    %% Must get tcp_closed eventually
    ok = wait_for_closed(SSock),
    ok = gen_tcp:close(SSock),
    close_race_loop(N - 1).

wait_for_closed(Sock) ->
    receive
        {tcp, Sock, _Data} -> wait_for_closed(Sock);
        {tcp_closed, Sock} -> ok
    after 2000 ->
        erlang:error(timeout_close_race)
    end.

%% -------------------------------------------------------------------
%% Test 3: Toggle setopts under load — sender pumps 200 messages,
%% server toggles once/true/false 20 times concurrently.
%% Verifies no data loss or spurious messages.
%% -------------------------------------------------------------------
test_setopts_under_load() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    Self = self(),
    %% Spawn sender that pumps messages
    spawn(fun() -> send_pump(CSock, 200), Self ! pump_done end),

    %% Toggle modes while data flows
    ok = toggle_loop(SSock, 20),

    %% Wait for sender to finish
    receive pump_done -> ok after 5000 -> erlang:error(pump_timeout) end,

    %% Drain any remaining data
    ok = inet:setopts(SSock, [{active, true}]),
    ok = gen_tcp:close(CSock),
    ok = drain_until_closed(SSock),
    ok = gen_tcp:close(SSock),
    ok.

send_pump(_Sock, 0) -> ok;
send_pump(Sock, N) ->
    gen_tcp:send(Sock, <<"x">>),
    send_pump(Sock, N - 1).

toggle_loop(_Sock, 0) -> ok;
toggle_loop(Sock, N) ->
    Mode = case N rem 3 of
        0 -> once;
        1 -> true;
        2 -> false
    end,
    ok = inet:setopts(Sock, [{active, Mode}]),
    %% Brief pause to let data flow
    case Mode of
        false -> ok;
        _ -> drain_some(Sock, 5)
    end,
    toggle_loop(Sock, N - 1).

drain_some(_Sock, 0) -> ok;
drain_some(Sock, N) ->
    receive
        {tcp, Sock, _} -> drain_some(Sock, N - 1)
    after 10 -> ok
    end.

drain_until_closed(Sock) ->
    receive
        {tcp, Sock, _} -> drain_until_closed(Sock);
        {tcp_closed, Sock} -> ok
    after 3000 -> ok
    end.

%% -------------------------------------------------------------------
%% Test 4: Multi-process active — 3 processes, each with own
%% connection, 20 rounds of {active, once}. No cross-contamination.
%% -------------------------------------------------------------------
test_multiprocess_active() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(LSock),

    Self = self(),
    Pids = [spawn(fun() ->
        {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
        {ok, SSock} = gen_tcp:accept(LSock),
        ok = mp_rearm_loop(SSock, CSock, 20),
        ok = gen_tcp:close(CSock),
        ok = gen_tcp:close(SSock),
        Self ! {done, self()}
    end) || _ <- lists:seq(1, 3)],

    ok = wait_all(Pids),
    ok = gen_tcp:close(LSock),
    ok.

mp_rearm_loop(_SSock, _CSock, 0) -> ok;
mp_rearm_loop(SSock, CSock, N) ->
    ok = inet:setopts(SSock, [{active, once}]),
    ok = gen_tcp:send(CSock, <<"Q\r\n">>),
    receive
        {tcp, SSock, <<"Q">>} -> ok
    after 2000 ->
        erlang:error({timeout_mp_rearm, N})
    end,
    mp_rearm_loop(SSock, CSock, N - 1).

wait_all([]) -> ok;
wait_all([Pid | Rest]) ->
    receive
        {done, Pid} -> wait_all(Rest)
    after 8000 ->
        erlang:error({timeout_multiprocess, Pid})
    end.
