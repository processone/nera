%% EXPECT: ok
-module(binary_last_match).
-export([test/0]).
test() ->
    255 = binary:last(<<0,255>>),
    0 = binary:last(<<0>>),
    Large = binary:copy(<<1,2,3>>, 100),
    3 = binary:last(binary:part(Large, 3, 90)),
    nomatch = binary:match(<<>>, <<"x">>),
    nomatch = binary:match(<<"a">>, <<"aa">>),
    {0,2} = binary:match(<<"aaa">>, <<"aa">>),
    {1,3} = binary:match(<<"xabcd">>, [<<"bcde">>, <<"ab">>, <<"abc">>]),
    {0,1} = binary:match(<<"abcde">>, [<<"bcde">>, <<"a">>]),
    {1,2} = binary:match(<<0,255,0>>, <<255,0>>),
    {0,2} = binary:match(binary:part(Large, 1, 80), [<<2>>, <<2,3>>]),
    lists:foreach(fun(B) -> badarg = failure(fun() -> binary:last(B) end) end,
        [<<>>, <<1:1>>, atom, [], 42]),
    lists:foreach(fun(P) ->
        badarg = failure(fun() -> binary:match(<<"a">>, P) end),
        badarg = failure(fun() -> binary:match(<<>>, P) end)
    end, [<<>>, [], [<<>>], [<<"a">>, <<>>], [<<"a">>|tail],
          [<<"a">>, atom], [[<<"a">>]], <<1:1>>, atom, 42]),
    badarg = failure(fun() -> binary:match(<<1:1>>, <<1>>) end),
    badarg = failure(fun() -> binary:match(atom, <<1>>) end),
    Last = fun binary:last/1,
    9 = Last(<<9>>),
    Match = fun binary:match/2,
    {1,1} = Match(<<"ab">>, <<"b">>),
    9 = apply(binary, last, [<<9>>]),
    {1,1} = apply(binary, match, [<<"ab">>, <<"b">>]),
    io:format("ok~n").
failure(F) -> try F() of _ -> unexpected_return catch error:R -> R end.
