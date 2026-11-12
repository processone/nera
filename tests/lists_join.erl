%% EXPECT: ok
-module(lists_join).
-export([test/0]).
test() ->
    [] = lists:join(separator, []),
    [only] = lists:join(separator, [only]),
    [a, separator, b, separator, c] = lists:join(separator, [a, b, c]),
    [[a], [sep], [b]] = lists:join([sep], [[a], [b]]),
    [a, [], b] = lists:join([], [a, b]),
    [<<"a">>, <<"&">>, <<"b">>] = lists:join(<<"&">>, [<<"a">>, <<"b">>]),
    [{x,1}, #{sep := true}, [2]] = lists:join(#{sep => true}, [{x,1}, [2]]),
    [] = apply(lists, join, [separator, []]),
    [a, dynamic, b] = apply(lists, join, [dynamic, [a, b]]),
    F = fun lists:join/2,
    [1, remote, 2] = F(remote, [1, 2]),
    function_clause = failure(fun() -> lists:join(sep, atom) end),
    function_clause = failure(fun() -> lists:join(sep, [a|tail]) end),
    function_clause = failure(fun() -> lists:join(sep, [a,b|tail]) end),
    Sep = [marker|tail],
    [a, Sep, b] = lists:join(Sep, [a,b]),
    Fun = fun(X) -> X end,
    [a, Fun, b] = lists:join(Fun, [a,b]),
    [<<5:3>>, sep, <<3:2>>] = lists:join(sep, [<<5:3>>, <<3:2>>]),
    function_clause = failure(fun() -> lists:join(sep, <<>>) end),
    function_clause = failure(fun() -> lists:join(sep, lists:seq(1,1000) ++ invalid_tail) end),
    lists:foreach(fun(N) ->
        Input = lists:seq(1,N),
        Output = lists:join(Sep, Input),
        ok = check_order(Input, Sep, Output),
        ExpectedLength = case N of 0 -> 0; _ -> 2*N-1 end,
        ExpectedLength = length(Output)
    end, lists:seq(0,128)),
    L = lists:seq(1, 100000),
    Joined = lists:join(sep, L),
    199999 = length(Joined),
    [1, sep, 2|_] = Joined,
    100000 = lists:last(Joined),
    io:format("ok~n").

failure(F) ->
    try F() of _ -> unexpected_return
    catch error:Reason -> Reason end.

check_order([], _, []) -> ok;
check_order([X], _, [X]) -> ok;
check_order([X,Y|Rest], Sep, [X,Sep|Joined]) ->
    check_order([Y|Rest], Sep, Joined).
