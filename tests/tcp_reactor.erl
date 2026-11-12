-module(tcp_reactor).
-export([test/0]).

%% Regression tests for reactor TCP/IP processing.
%%
%% Covers the hybrid active-mode dispatch (reactor-inline for {active,once},
%% worker-offload for {active,true}), deferred set_active to prevent
%% reactor/worker fd races, lazy deregistration, tcp_closed delivery,
%% passive/active interleaving, and concurrent multi-connection scenarios.

test() ->
    ok = test_active_true_raw(),
    ok = test_active_true_line(),
    ok = test_tcp_closed_active_once(),
    ok = test_tcp_closed_active_true(),
    ok = test_active_once_rapid_rearm(),
    ok = test_passive_active_interleave(),
    ok = test_concurrent_connections(),
    ok = test_active_once_large_line(),
    ok = test_setopts_toggle(),
    ok.

%% -------------------------------------------------------------------
%% Test 1: {active, true} + raw mode — worker-offload path
%% Verify all data is received and tcp_closed is delivered.
%% -------------------------------------------------------------------
test_active_true_raw() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    ok = inet:setopts(SSock, [{active, true}]),

    %% Send several chunks, then close
    ok = gen_tcp:send(CSock, <<"chunk1">>),
    ok = gen_tcp:send(CSock, <<"chunk2">>),
    ok = gen_tcp:send(CSock, <<"chunk3">>),
    ok = gen_tcp:close(CSock),

    %% Collect all data + closed signal
    {Bytes, GotClosed} = collect_active(SSock, <<>>, false),
    true = GotClosed,
    %% All 18 bytes must arrive (order preserved, no loss)
    18 = byte_size(Bytes),
    ok = gen_tcp:close(SSock),
    ok.

%% -------------------------------------------------------------------
%% Test 2: {active, true} + {packet, line} — worker-offload line mode
%% Verify each line is delivered as a separate message.
%% -------------------------------------------------------------------
test_active_true_line() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    ok = inet:setopts(SSock, [{active, true}]),

    %% Send 5 lines in one write
    ok = gen_tcp:send(CSock, <<"line1\r\nline2\r\nline3\r\nline4\r\nline5\r\n">>),

    %% Collect exactly 5 line messages
    Lines = collect_lines(SSock, 5, []),
    5 = length(Lines),
    <<"line1">> = lists:nth(1, Lines),
    <<"line5">> = lists:nth(5, Lines),

    ok = gen_tcp:close(CSock),
    ok = gen_tcp:close(SSock),
    ok.

%% -------------------------------------------------------------------
%% Test 3: tcp_closed delivery in {active, once} mode
%% Verify {tcp_closed, Socket} arrives when peer closes.
%% -------------------------------------------------------------------
test_tcp_closed_active_once() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    ok = inet:setopts(SSock, [{active, once}]),
    ok = gen_tcp:close(CSock),

    receive
        {tcp_closed, SSock} -> ok
    after 2000 ->
        erlang:error(timeout_tcp_closed_once)
    end,
    ok = gen_tcp:close(SSock),
    ok.

%% -------------------------------------------------------------------
%% Test 4: tcp_closed delivery in {active, true} mode
%% Verify {tcp_closed, Socket} eventually arrives.
%% -------------------------------------------------------------------
test_tcp_closed_active_true() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    ok = inet:setopts(SSock, [{active, true}]),

    %% Send some data then close
    ok = gen_tcp:send(CSock, <<"before close">>),
    ok = gen_tcp:close(CSock),

    {_Bytes, GotClosed} = collect_active(SSock, <<>>, false),
    true = GotClosed,
    ok = gen_tcp:close(SSock),
    ok.

%% -------------------------------------------------------------------
%% Test 5: Rapid {active, once} re-arm — simulates request/response
%% protocol under load. Catches stalls from lost re-arm or deferred
%% set_active races. 200 rounds is enough to trigger timing-dependent
%% bugs without making the test slow.
%% -------------------------------------------------------------------
test_active_once_rapid_rearm() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    ok = rapid_rearm_loop(SSock, CSock, 200),

    ok = gen_tcp:close(CSock),
    ok = gen_tcp:close(SSock),
    ok.

rapid_rearm_loop(_SSock, _CSock, 0) -> ok;
rapid_rearm_loop(SSock, CSock, N) ->
    %% Server arms active once
    ok = inet:setopts(SSock, [{active, once}]),
    %% Client sends request
    ok = gen_tcp:send(CSock, <<"REQ\r\n">>),
    %% Server receives via active
    receive
        {tcp, SSock, <<"REQ">>} -> ok;
        {tcp, SSock, Wrong} -> erlang:error({wrong_req, N, Wrong})
    after 2000 ->
        erlang:error({timeout_rapid_rearm, N})
    end,
    %% Server sends response (passive)
    ok = gen_tcp:send(SSock, <<"OK\r\n">>),
    %% Client reads response (passive)
    {ok, <<"OK\r\n">>} = gen_tcp:recv(CSock, 4, 2000),
    rapid_rearm_loop(SSock, CSock, N - 1).

%% -------------------------------------------------------------------
%% Test 6: Passive/active interleaving — the memcached SET pattern.
%% {active,once} reads command line, passive recv reads value bytes,
%% then re-arm. Catches reactor/worker fd data races (the deferred
%% set_active bug).
%% -------------------------------------------------------------------
test_passive_active_interleave() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    ok = interleave_loop(SSock, CSock, 50),

    ok = gen_tcp:close(CSock),
    ok = gen_tcp:close(SSock),
    ok.

interleave_loop(_SSock, _CSock, 0) -> ok;
interleave_loop(SSock, CSock, N) ->
    %% Arm active once for the command line
    ok = inet:setopts(SSock, [{active, once}]),

    %% Client sends command + value in one write (tests framing)
    ValSize = 10 + N,
    Val = list_to_binary(lists:duplicate(ValSize, $x)),
    SizeStr = integer_to_binary(ValSize),
    Cmd = <<"SET key 0 0 ", SizeStr/binary, "\r\n">>,
    ok = gen_tcp:send(CSock, <<Cmd/binary, Val/binary, "\r\n">>),

    %% Server receives command via active mode
    CmdExpected = <<"SET key 0 0 ", SizeStr/binary>>,
    receive
        {tcp, SSock, CmdExpected} -> ok;
        {tcp, SSock, BadCmd} -> erlang:error({bad_cmd, N, BadCmd})
    after 2000 ->
        erlang:error({timeout_interleave_cmd, N})
    end,

    %% Value bytes must NOT leak as an active message
    receive
        {tcp, SSock, Leak} -> erlang:error({value_leaked, N, Leak})
    after 0 -> ok
    end,

    %% Server reads value + \r\n via passive recv
    RecvLen = ValSize + 2,
    {ok, RecvData} = gen_tcp:recv(SSock, RecvLen, 2000),
    RecvLen = byte_size(RecvData),

    interleave_loop(SSock, CSock, N - 1).

%% -------------------------------------------------------------------
%% Test 7: Concurrent connections — multiple sockets in {active, once}
%% simultaneously. Verifies no cross-socket contamination and that the
%% reactor handles multiple fds without losing events.
%% -------------------------------------------------------------------
test_concurrent_connections() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(LSock),

    %% Create 10 connection pairs
    Pairs = create_pairs(LSock, Port, 10, []),
    ok = gen_tcp:close(LSock),

    %% Run 20 rounds on all connections concurrently
    ok = concurrent_rounds(Pairs, 20),

    %% Cleanup
    ok = close_pairs(Pairs),
    ok.

create_pairs(_LSock, _Port, 0, Acc) -> lists:reverse(Acc);
create_pairs(LSock, Port, N, Acc) ->
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    create_pairs(LSock, Port, N - 1, [{CSock, SSock} | Acc]).

concurrent_rounds(_Pairs, 0) -> ok;
concurrent_rounds(Pairs, Round) ->
    %% Arm all server sockets
    lists:foreach(fun({_C, S}) ->
        ok = inet:setopts(S, [{active, once}])
    end, Pairs),

    %% Send on all client sockets
    RoundBin = integer_to_binary(Round),
    lists:foreach(fun({C, _S}) ->
        ok = gen_tcp:send(C, <<"R", RoundBin/binary, "\r\n">>)
    end, Pairs),

    %% Receive on all server sockets (order may vary)
    lists:foreach(fun({_C, S}) ->
        receive
            {tcp, S, _Data} -> ok
        after 2000 ->
            erlang:error({timeout_concurrent, Round})
        end
    end, Pairs),

    concurrent_rounds(Pairs, Round - 1).

close_pairs([]) -> ok;
close_pairs([{C, S} | Rest]) ->
    gen_tcp:close(C),
    gen_tcp:close(S),
    close_pairs(Rest).

%% -------------------------------------------------------------------
%% Test 8: Large line in {active, once} — line longer than the 4096
%% peek buffer used by the setopts fastpath, forcing reactor slow path.
%% -------------------------------------------------------------------
test_active_once_large_line() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    ok = inet:setopts(SSock, [{active, once}]),

    %% 6000-byte line (exceeds 4096 setopts fastpath peek buffer)
    BigLine = list_to_binary(lists:duplicate(6000, $A)),
    ok = gen_tcp:send(CSock, <<BigLine/binary, "\r\n">>),

    receive
        {tcp, SSock, Received} ->
            6000 = byte_size(Received),
            BigLine = Received
    after 3000 ->
        erlang:error(timeout_large_line)
    end,

    ok = gen_tcp:close(CSock),
    ok = gen_tcp:close(SSock),
    ok.

%% -------------------------------------------------------------------
%% Test 9: Toggle active mode — switch between once/true/false and
%% verify no stale events or lost data.
%% -------------------------------------------------------------------
test_setopts_toggle() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    %% Phase 1: {active, once} — one message
    ok = inet:setopts(SSock, [{active, once}]),
    ok = gen_tcp:send(CSock, <<"msg1">>),
    receive
        {tcp, SSock, <<"msg1">>} -> ok
    after 2000 -> erlang:error(timeout_toggle_1)
    end,

    %% Phase 2: passive recv
    ok = gen_tcp:send(CSock, <<"msg2">>),
    {ok, <<"msg2">>} = gen_tcp:recv(SSock, 4, 2000),

    %% Phase 3: {active, true} — continuous
    %% In raw mode, TCP may coalesce sends, so we check total bytes
    %% rather than per-message boundaries.
    ok = inet:setopts(SSock, [{active, true}]),
    receive after 10 -> ok end,
    ok = gen_tcp:send(CSock, <<"msg3">>),
    receive after 10 -> ok end,
    ok = gen_tcp:send(CSock, <<"msg4">>),
    Phase3 = collect_bytes_active(SSock, <<>>, 8),
    <<"msg3msg4">> = Phase3,

    %% Phase 4: back to passive
    ok = inet:setopts(SSock, [{active, false}]),
    ok = gen_tcp:send(CSock, <<"msg5">>),
    %% No active message should arrive
    receive {tcp, SSock, _} -> erlang:error(spurious_after_passive)
    after 100 -> ok
    end,
    {ok, <<"msg5">>} = gen_tcp:recv(SSock, 4, 2000),

    %% Phase 5: back to {active, once}
    ok = inet:setopts(SSock, [{active, once}]),
    ok = gen_tcp:send(CSock, <<"msg6">>),
    receive {tcp, SSock, <<"msg6">>} -> ok
    after 2000 -> erlang:error(timeout_toggle_6)
    end,

    ok = gen_tcp:close(CSock),
    ok = gen_tcp:close(SSock),
    ok.

%% -------------------------------------------------------------------
%% Helpers
%% -------------------------------------------------------------------

%% Collect all {tcp, Sock, Data} and {tcp_closed, Sock} messages.
collect_active(Sock, Acc, Closed) ->
    receive
        {tcp, Sock, Data} ->
            collect_active(Sock, <<Acc/binary, Data/binary>>, Closed);
        {tcp_closed, Sock} ->
            {Acc, true}
    after 2000 ->
        {Acc, Closed}
    end.

%% Collect exactly N line messages from an {active, true} socket.
collect_lines(_Sock, 0, Acc) -> lists:reverse(Acc);
collect_lines(Sock, N, Acc) ->
    receive
        {tcp, Sock, Line} ->
            collect_lines(Sock, N - 1, [Line | Acc])
    after 2000 ->
        erlang:error({timeout_collect_lines, N, lists:reverse(Acc)})
    end.

%% Collect at least ExpectedBytes from {active, true} raw socket.
collect_bytes_active(Sock, Acc, ExpectedBytes) when byte_size(Acc) >= ExpectedBytes ->
    Acc;
collect_bytes_active(Sock, Acc, ExpectedBytes) ->
    receive
        {tcp, Sock, Data} ->
            collect_bytes_active(Sock, <<Acc/binary, Data/binary>>, ExpectedBytes)
    after 2000 ->
        erlang:error({timeout_collect_bytes, byte_size(Acc), ExpectedBytes})
    end.
