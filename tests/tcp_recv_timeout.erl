-module(tcp_recv_timeout).
-export([test/0]).
%% EXPECT: recv_timeout_ok

%% Regression for recv/3 ignoring its finite timeout on an idle connection.
test() ->
    {ok, L} = gen_tcp:listen(0, [binary, {active, false}]),
    {ok, Port} = inet:port(L),
    {ok, C} = gen_tcp:connect("127.0.0.1", Port, [binary, {active, false}]),
    {ok, S} = gen_tcp:accept(L),
    timeout_recv(S, 0, 60),
    timeout_recv(S, 4, 60),
    timeout_recv(S, 0, 0),
    timeout_recv(S, 4, 0),
    ok = gen_tcp:send(C, <<"poll">>),
    timeout_recv(S, 5, 60),
    {ok, <<"poll">>} = gen_tcp:recv(S, 4, 0),
    Payload = binary:copy(<<7>>, 8192),
    ok = gen_tcp:send(C, Payload),
    timeout_recv(S, 8193, 60),
    {ok, Payload} = gen_tcp:recv(S, 8192, 0),
    ok = gen_tcp:send(C, <<"ab">>),
    timeout_recv(S, 4, 60),
    ok = gen_tcp:send(C, <<"cd">>),
    {ok, <<"abcd">>} = gen_tcp:recv(S, 4, 1000),
    ok = gen_tcp:send(C, <<"xy">>),
    timeout_recv(S, 4, 60),
    {ok, <<"xy">>} = gen_tcp:recv(S, 0, 0),
    %% The total deadline must not restart when another fragment arrives.
    spawn_link(fun() ->
        receive after 100 -> ok end,
        ok = gen_tcp:send(C, <<1>>),
        receive after 150 -> ok end,
        ok = gen_tcp:send(C, <<2, 3, 4>>)
    end),
    timeout_recv(S, 4, 200),
    {ok, <<1, 2, 3, 4>>} = gen_tcp:recv(S, 4, infinity),
    Parent = self(),
    spawn_link(fun() ->
        receive after 20 -> ok end,
        ok = gen_tcp:send(C, <<"ready">>),
        Parent ! sent
    end),
    {ok, <<"ready">>} = gen_tcp:recv(S, 5, 1000),
    receive sent -> ok after 2000 -> fail(sender) end,
    %% A completed receive's deadline must not wake the next receive.
    spawn_link(fun() ->
        receive after 1100 -> ok end,
        ok = gen_tcp:send(C, <<"later">>)
    end),
    {ok, <<"later">>} = gen_tcp:recv(S, 5, infinity),
    ok = gen_tcp:send(C, <<"two">>),
    {ok, <<"two">>} = gen_tcp:recv(S, 3),
    ok = gen_tcp:close(C),
    ok = gen_tcp:close(S),
    ok = gen_tcp:close(L),
    io:format("recv_timeout_ok~n"),
    ok.

timeout_recv(S, Length, Timeout) ->
    Start = erlang:monotonic_time(millisecond),
    {error, timeout} = gen_tcp:recv(S, Length, Timeout),
    Elapsed = erlang:monotonic_time(millisecond) - Start,
    case Elapsed >= Timeout andalso Elapsed < Timeout + 2000 of
        true -> io:format("idle length=~p timeout=~p elapsed=~p~n", [Length, Timeout, Elapsed]);
        false -> fail({elapsed, Length, Timeout, Elapsed})
    end.

fail(Reason) -> erlang:error({recv_timeout_failed, Reason}).
