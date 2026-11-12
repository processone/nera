-module(tcp_argument_domain).
-export([test/0]).
test() ->
    lists:foreach(fun(P) ->
        bad(fun() -> gen_tcp:listen(P, [binary, {active, false}]) end),
        bad(fun() -> gen_tcp:connect("127.0.0.1", P, [binary, {active, false}]) end)
    end, [-1, 65536, 1152921504606846975, 1152921504606846976, invalid, []]),
    {ok, L} = gen_tcp:listen(0, [binary, {active, false}]),
    {ok, Port} = inet:port(L),
    {ok, C} = gen_tcp:connect("127.0.0.1", Port, [binary, {active, false}]),
    {ok, S} = gen_tcp:accept(L),
    ok = gen_tcp:send(C, <<42>>),
    lists:foreach(fun(N) ->
        bad(fun() -> gen_tcp:recv(S, N) end),
        bad(fun() -> gen_tcp:recv(S, N, infinity) end)
    end,
        [-1, invalid, [], 1152921504606846976]),
    {ok, <<42>>} = gen_tcp:recv(S, 1),
    ok = gen_tcp:send(C, <<43>>), {ok, <<43>>} = gen_tcp:recv(S, 0),
    Parent = self(), Payload = binary:copy(<<7>>, 1048576),
    spawn(fun() -> ok = gen_tcp:send(C, Payload), Parent ! sent end),
    {ok, Payload} = gen_tcp:recv(S, 1048576), receive sent -> ok end,
    ok = gen_tcp:close(C),
    %% The existing inline EOF path uses closed; reactor EOF maps to einval.
    %% Both must return without reserving the requested enormous length.
    case gen_tcp:recv(S, 1152921504606846975) of
        {error, closed} -> ok;
        {error, einval} -> ok
    end,
    ok = gen_tcp:close(S), ok = gen_tcp:close(L), ok.
bad(F) -> {'EXIT', {badarg, _}} = (catch F()), ok.
