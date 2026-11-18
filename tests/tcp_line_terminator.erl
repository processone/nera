%% EXPECT: tcp_line_terminator_ok
%% An active socket in {packet, line} mode delivers each line with its
%% terminator, "\r\n" or "\n", as BEAM does. Covers {active, once} when
%% data arrives after setopts and when it is already waiting, {active, true}
%% with several lines and empty lines in one write, lines split across
%% writes (one split between "\r" and "\n"), and a partial line completed
%% by a later write. A passive recv after {active, once} reads the rest.
%% The pauses only let earlier bytes reach the server first; the expected
%% messages are the same without them. Run this same fixture on native
%% Nera and BEAM.
-module(tcp_line_terminator).
-export([test/0]).

test() ->
    ok = with_pair(fun once_then_passive/2),
    ok = with_pair(fun once_data_already_waiting/2),
    ok = with_pair(fun true_several_lines/2),
    ok = with_pair(fun true_split_lines/2),
    ok = with_pair(fun once_partial_line_completed/2),
    io:format("tcp_line_terminator_ok~n").

%% {active, once} armed before the data arrives: only the first line is
%% delivered, then a passive recv returns the second.
once_then_passive(S, C) ->
    ok = inet:setopts(S, [{active, once}]),
    ok = gen_tcp:send(C, <<"P\r\nQ\n">>),
    <<"P\r\n">> = next(S),
    none = next(S, 100),
    {ok, <<"Q\n">>} = gen_tcp:recv(S, 0, 2000),
    ok.

%% {active, once} armed after both lines are waiting.
once_data_already_waiting(S, C) ->
    ok = gen_tcp:send(C, <<"L1\r\nL2\n">>),
    pause(),
    ok = inet:setopts(S, [{active, once}]),
    <<"L1\r\n">> = next(S),
    ok = inet:setopts(S, [{active, once}]),
    <<"L2\n">> = next(S),
    none = next(S, 100),
    ok.

%% {active, true}: one message per line, empty lines included.
true_several_lines(S, C) ->
    ok = inet:setopts(S, [{active, true}]),
    ok = gen_tcp:send(C, <<"a\r\nb\n\n\r\nc\r\n">>),
    [<<"a\r\n">>, <<"b\n">>, <<"\n">>, <<"\r\n">>, <<"c\r\n">>] = take(S, 5),
    none = next(S, 100),
    ok.

%% {active, true}: a line reaches the server in three writes, the last
%% split falling between "\r" and "\n".
true_split_lines(S, C) ->
    ok = inet:setopts(S, [{active, true}]),
    ok = gen_tcp:send(C, <<"AB">>),
    pause(),
    ok = gen_tcp:send(C, <<"C\r">>),
    pause(),
    ok = gen_tcp:send(C, <<"\nD\n">>),
    [<<"ABC\r\n">>, <<"D\n">>] = take(S, 2),
    none = next(S, 100),
    ok.

%% {active, once}: a partial line is waiting when the socket is armed;
%% the next write completes it.
once_partial_line_completed(S, C) ->
    ok = gen_tcp:send(C, <<"abc">>),
    pause(),
    ok = inet:setopts(S, [{active, once}]),
    pause(),
    ok = gen_tcp:send(C, <<"def\r\nghi\n">>),
    <<"abcdef\r\n">> = next(S),
    ok = inet:setopts(S, [{active, once}]),
    <<"ghi\n">> = next(S),
    none = next(S, 100),
    ok.

with_pair(F) ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary, {packet, line}]),
    {ok, Port} = inet:port(LSock),
    {ok, C} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, S} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),
    ok = F(S, C),
    ok = gen_tcp:close(C),
    ok = gen_tcp:close(S),
    ok.

next(S) ->
    case next(S, 2000) of
        none -> erlang:error({no_line, S});
        Data -> Data
    end.

next(S, Timeout) ->
    receive
        {tcp, S, Data} -> Data;
        {tcp_closed, S} -> erlang:error({tcp_closed, S});
        {tcp_error, S, Reason} -> erlang:error({tcp_error, Reason})
    after Timeout -> none
    end.

take(_S, 0) -> [];
take(S, N) -> [next(S) | take(S, N - 1)].

pause() ->
    receive after 50 -> ok end.
