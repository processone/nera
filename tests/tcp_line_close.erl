%% EXPECT: tcp_line_close_ok
%% BEAM drops an unterminated final packet on an active line socket. A
%% complete packet preceding that tail still arrives before tcp_closed.
-module(tcp_line_close).
-export([test/0]).

test() ->
    AfterLine = close_messages(true_after_line, true, <<"STATUS\r\nQUIT">>, before_send),
    TrailingCR = close_messages(true_trailing_cr, true, <<"QUIT\r">>, before_send),
    OnceBeforeSend = close_messages(once_before_send, once, <<"NOOP">>, before_send),
    OnceAfterClose = close_messages(once_after_close, once, <<"PING">>, after_close),
    [{tcp, <<"STATUS\r\n">>}, tcp_closed] = AfterLine,
    [tcp_closed] = TrailingCR,
    [tcp_closed] = OnceBeforeSend,
    [tcp_closed] = OnceAfterClose,
    io:format("tcp_line_close_ok~n").

close_messages(Name, Mode, Bytes, Arm) ->
    {ok, Listener} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(Listener),
    {ok, Peer} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, Socket} = gen_tcp:accept(Listener),
    ok = gen_tcp:close(Listener),
    ok = maybe_arm(Socket, Mode, Arm, before_send),
    ok = gen_tcp:send(Peer, Bytes),
    ok = gen_tcp:close(Peer),
    ok = maybe_arm(Socket, Mode, Arm, after_close),
    Messages = until_close(Socket, []),
    io:format("~s: ~w~n", [Name, Messages]),
    ok = gen_tcp:close(Socket),
    Messages.

maybe_arm(Socket, Mode, When, When) -> inet:setopts(Socket, [{active, Mode}]);
maybe_arm(_Socket, _Mode, _Arm, _When) -> ok.

until_close(Socket, Acc) ->
    receive
        {tcp, Socket, Data} -> until_close(Socket, [{tcp, Data} | Acc]);
        {tcp_closed, Socket} -> lists:reverse([tcp_closed | Acc]);
        {tcp_error, Socket, Reason} -> lists:reverse([{tcp_error, Reason} | Acc])
    after 2000 ->
        lists:reverse([timeout | Acc])
    end.
