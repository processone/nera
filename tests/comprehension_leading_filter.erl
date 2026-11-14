%% EXPECT: comprehension_leading_filter_ok
%% Leading filters must run before any generator source, and filter-only
%% comprehensions produce their body once or not at all, as on BEAM.
%% Run this same fixture on native Nera and BEAM.
-module(comprehension_leading_filter).
-export([test/0]).

test() ->
    [] = [X || false, X <- [1, 2]],
    [ok] = [ok || true],
    [] = [ok || false],
    <<>> = << <<X>> || false, X <- [1, 2] >>,
    <<7:3>> = << <<7:3>> || true >>,
    <<>> = << <<7:3>> || false >>,
    lists:foreach(fun(Flag) ->
        Expected = case Flag of true -> [1, 2]; false -> [] end,
        Expected = [X || Flag, X <- id([1, 2])],
        Expected = [X || Flag, <<X>> <= id(<<1, 2>>)],
        ExpectedBits = case Flag of true -> <<1:3, 2:3>>; false -> <<>> end,
        ExpectedBits = << <<X:3>> || Flag, X <- id([1, 2]) >>,
        ExpectedBits = << <<X:3>> || Flag, <<X>> <= id(<<1, 2>>) >>,
        ExpectedOne = case Flag of true -> [ok]; false -> [] end,
        ExpectedOne = [ok || Flag, id(true)],
        ExpectedByte = case Flag of true -> <<42>>; false -> <<>> end,
        ExpectedByte = << <<42>> || Flag, id(true) >>
    end, [true, false]),
    traces(),
    captures(),
    exceptions(),
    io:format("comprehension_leading_filter_ok~n"),
    ok.

traces() ->
    {[], [first, stop]} = traced(fun() ->
        [event(body, X) || event(first, true), event(stop, false),
            event(skipped, true), X <- event(source, [1, 2])]
    end),
    {<<>>, [stop]} = traced(fun() ->
        << (event(body, <<X>>)) || event(stop, false),
            <<X>> <= event(source, <<1, 2>>) >>
    end),
    {[ok], [first, second, body]} = traced(fun() ->
        [event(body, ok) || event(first, true), event(second, true)]
    end),
    {<<1:3>>, [first, body]} = traced(fun() ->
        << (event(body, <<1:3>>)) || event(first, true) >>
    end),
    {[11, 21, 12, 22], [first, source, inner, body, body, inner, body, body]} =
        traced(fun() ->
            [event(body, X + Y) || event(first, true), X <- event(source, [1, 2]),
                Y <- event(inner, [10, 20])]
        end),
    {<<1, 2>>, [first, source, body, body]} = traced(fun() ->
        << (event(body, <<X>>)) || event(first, true),
            <<X>> <= event(source, <<1, 2>>) >>
    end),
    ok.

captures() ->
    Outer = id(2),
    [1, 2] = [Outer || Outer =:= 2, Outer <- id([1, 2])],
    <<1, 2>> = << <<Outer>> || Outer =:= 2, <<Outer>> <= id(<<1, 2>>) >>,
    <<0, Tail/bits>> = id(<<0, 5:3>>),
    [Tail] = [Tail || bit_size(Tail) =:= 3],
    Tail = << Tail || bit_size(Tail) =:= 3 >>,
    3 = bit_size(Tail),
    [3] = [BodyBound || begin BodyBound = id(3), BodyBound > 0 end],
    [4] = [SourceBound + X || begin SourceBound = id(3), SourceBound > 0 end,
        X <- id([1])],
    <<4>> = << <<(BitsBound + X)>> || begin BitsBound = id(3), BitsBound > 0 end,
        <<X>> <= id(<<1>>) >>,
    [[1], [2]] = [[X || X > 0] || X <- id([1, 2])],
    ok.

exceptions() ->
    [] = [error(body_ran) || id(false)],
    <<>> = << (id(not_bits)) || id(false) >>,
    [] = [X || id(false), X <- error(source_ran)],
    <<>> = << <<X>> || id(false), <<X>> <= error(source_ran) >>,
    {{throw, filter_failed}, [first]} = traced(fun() ->
        [event(body, X) || event(first, true), throw(filter_failed),
            X <- event(source, [1])]
    end),
    {{throw, filter_failed}, [first]} = traced(fun() ->
        << (event(body, <<1>>)) || event(first, true), throw(filter_failed) >>
    end),
    {error, badarg} = result(fun() -> << (id(not_bits)) || id(true) >> end),
    badarg = try
        << (id(not_bits)) || id(true) >>,
        error(after_bad_body)
    catch error:BodyReason -> BodyReason end,
    filter_failed = try
        [ok || throw(filter_failed)],
        error(after_bad_filter)
    catch throw:FilterReason -> FilterReason end,
    ok.

traced(Fun) ->
    put(trace, []),
    Result = result(Fun),
    {Result, lists:reverse(erase(trace))}.

result(Fun) ->
    try Fun() catch Class:Reason -> {Class, Reason} end.

event(Name, Value) -> put(trace, [Name | get(trace)]), Value.

id(X) -> X.
