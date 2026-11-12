-module(tcp_active_once).
-export([test/0]).

%% Regression test for hybrid active-mode TCP dispatch.
%% Verifies that {active, once} with {packet, line} correctly frames
%% multi-part protocols (command line + value bytes), and that value
%% bytes are not delivered as active messages.

test() ->
    ok = test_active_once_line_framing(),
    ok = test_active_once_raw_framing(),
    ok = test_active_once_multiple_rounds(),
    ok.

%% Test 1: {active, once} + {packet, line} — send a command line followed
%% by value bytes. Verify the command is delivered as an active message
%% and the value bytes are read via gen_tcp:recv (passive).
test_active_once_line_framing() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    %% Server side: set {active, once}
    ok = inet:setopts(SSock, [{active, once}]),

    %% Client sends command line + value bytes in one write
    ok = gen_tcp:send(CSock, <<"SET key 0 0 5\r\nhello\r\n">>),

    %% Server should receive ONLY the command line as active message
    receive
        {tcp, _, <<"SET key 0 0 5">>} -> ok;
        {tcp, _, Other} -> erlang:error({unexpected_active_data, Other})
    after 2000 ->
        erlang:error(timeout_waiting_for_active_msg)
    end,

    %% Value bytes must NOT arrive as another active message
    receive
        {tcp, _, Spurious} -> erlang:error({spurious_active_msg, Spurious})
    after 100 ->
        ok  %% Good — no spurious message
    end,

    %% Read value bytes via passive gen_tcp:recv
    {ok, <<"hello\r\n">>} = gen_tcp:recv(SSock, 7, 2000),

    ok = gen_tcp:close(CSock),
    ok = gen_tcp:close(SSock),
    ok.

%% Test 2: {active, once} + {packet, raw} — verify single read delivery.
test_active_once_raw_framing() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    ok = inet:setopts(SSock, [{active, once}]),

    %% Send data
    ok = gen_tcp:send(CSock, <<"hello world">>),

    %% Should receive one active message
    receive
        {tcp, _, Data} when byte_size(Data) > 0 -> ok
    after 2000 ->
        erlang:error(timeout_raw_active)
    end,

    %% No second active message (once mode)
    ok = gen_tcp:send(CSock, <<"more data">>),
    receive
        {tcp, _, _Spurious2} -> erlang:error(spurious_raw_active_msg)
    after 100 ->
        ok
    end,

    %% But passive recv should work
    {ok, <<"more data">>} = gen_tcp:recv(SSock, 9, 2000),

    ok = gen_tcp:close(CSock),
    ok = gen_tcp:close(SSock),
    ok.

%% Test 3: Multiple {active, once} rounds — simulates a request/response
%% protocol like memcached over several cycles.
test_active_once_multiple_rounds() ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),

    ok = multi_round_loop(SSock, CSock, 20),

    ok = gen_tcp:close(CSock),
    ok = gen_tcp:close(SSock),
    ok.

multi_round_loop(_SSock, _CSock, 0) -> ok;
multi_round_loop(SSock, CSock, N) ->
    %% Arm active once
    ok = inet:setopts(SSock, [{active, once}]),

    %% Client sends command + value
    Cmd = <<"CMD ", (integer_to_binary(N))/binary, "\r\n">>,
    Val = <<"VAL", (integer_to_binary(N))/binary, "\r\n">>,
    ok = gen_tcp:send(CSock, <<Cmd/binary, Val/binary>>),

    %% Server receives command via active (line mode strips \r\n)
    CmdExpected = <<"CMD ", (integer_to_binary(N))/binary>>,
    receive
        {tcp, _, Received} ->
            case Received =:= CmdExpected of
                true -> ok;
                false -> erlang:error({wrong_cmd, N, Received, CmdExpected})
            end
    after 2000 ->
        erlang:error({timeout_cmd, N})
    end,

    %% Server reads value via passive recv
    ValLen = byte_size(Val),
    {ok, Val} = gen_tcp:recv(SSock, ValLen, 2000),

    multi_round_loop(SSock, CSock, N - 1).
