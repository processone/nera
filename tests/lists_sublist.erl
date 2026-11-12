%% EXPECT: ok
-module(lists_sublist).
-export([test/0]).
test() ->
    %% Dynamic dispatch keeps the regression executable before the API exists:
    %% an undef is observed as a value and fails this prefix assertion.
    [a,b] = result([a,b,c], 2),
    [a,b] = lists:sublist([a,b,c], 2),
    F = fun lists:sublist/2,
    [a] = F([a,b], 1),
    [] = result([], 0),
    [] = result([], 10),
    [] = result([a,b], 0),
    [a,b] = result([a,b], 20),
    [a,b] = result([a,b], 2),
    [{a,1}, <<2>>, [c]] = result([{a,1}, <<2>>, [c], d], 3),
    %% Only the requested prefix is consumed, including improper lists.
    [] = result([a|tail], 0),
    [a] = result([a|tail], 1),
    {exception,function_clause} = result([a|tail], 2),
    lists:foreach(fun({L,N}) ->
        {exception,function_clause} = result(L,N)
    end, [{[], -1}, {[a], -1}, {[a], 1.0}, {[a], count},
          {atom, 0}, {<<>>, 0}, {atom, 2}]),
    Long = lists:seq(1,100000),
    Prefix = result(Long,99999),
    99999 = length(Prefix),
    99999 = lists:last(Prefix),
    Long = result(Long,100001),
    io:format("ok~n").

result(List, Count) ->
    try apply(lists, sublist, [List, Count]) of
        Value -> Value
    catch error:Reason -> {exception,Reason} end.
