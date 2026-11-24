-module(tcp_recv_timeout_active).
-export([test/0]).
%% EXPECT: recv_timeout_active_ok

%% Retained bytes must remain readable when timeout recovery switches to active mode.
test() ->
    raw(true),
    raw(once),
    lines(),
    split_line(),
    io:format("recv_timeout_active_ok~n"),
    ok.

raw(Mode) ->
    {L, C, S} = pair(),
    Payload = binary:copy(<<"0123456789abcdef">>, 1024),
    retain(C, S, Payload),
    ok = inet:setopts(S, [{active, Mode}]),
    Recovered = chunks(S, Mode, byte_size(Payload), <<>>),
    Payload = Recovered,
    io:format("raw active=~p recovered=~p bytes~n", [Mode, byte_size(Recovered)]),
    close(L, C, S).

chunks(_S, _Mode, 0, Acc) -> Acc;
chunks(S, Mode, Remaining, Acc) ->
    receive
        {tcp, S, Data} when byte_size(Data) =< Remaining ->
            Left = Remaining - byte_size(Data),
            case Mode =:= once andalso Left > 0 of
                true -> ok = inet:setopts(S, [{active, once}]);
                false -> ok
            end,
            chunks(S, Mode, Left, <<Acc/binary, Data/binary>>)
    after 2000 -> erlang:error({retained_bytes_stalled, Mode, Remaining})
    end.

lines() ->
    {L, C, S} = pair(),
    First = <<(binary:copy(<<"x">>, 20000))/binary, "\r\n">>,
    retain(C, S, <<First/binary, "second\nthird\r\n">>),
    ok = inet:setopts(S, [{packet, line}, {active, once}]),
    FirstRead = line(S),
    First = FirstRead,
    ok = inet:setopts(S, [{active, true}]),
    SecondRead = line(S),
    <<"second\n">> = SecondRead,
    ThirdRead = line(S),
    <<"third\r\n">> = ThirdRead,
    io:format("line recovered=~p bytes then ~p and ~p~n", [byte_size(FirstRead), SecondRead, ThirdRead]),
    close(L, C, S).

split_line() ->
    {L, C, S} = pair(),
    retain(C, S, <<"kept">>),
    ok = inet:setopts(S, [{packet, line}, {active, once}]),
    ok = gen_tcp:send(C, <<"tail\n">>),
    Recovered = line(S),
    <<"kepttail\n">> = Recovered,
    io:format("split line recovered=~p~n", [Recovered]),
    close(L, C, S).

line(S) ->
    receive {tcp, S, Data} -> Data
    after 2000 -> erlang:error(retained_line_stalled)
    end.

retain(C, S, Payload) ->
    ok = gen_tcp:send(C, Payload),
    {error, timeout} = gen_tcp:recv(S, byte_size(Payload) + 1, 60).

pair() ->
    {ok, L} = gen_tcp:listen(0, [binary, {active, false}]),
    {ok, Port} = inet:port(L),
    {ok, C} = gen_tcp:connect("127.0.0.1", Port, [binary, {active, false}]),
    {ok, S} = gen_tcp:accept(L),
    {L, C, S}.

close(L, C, S) ->
    ok = gen_tcp:close(C),
    ok = gen_tcp:close(S),
    ok = gen_tcp:close(L).
