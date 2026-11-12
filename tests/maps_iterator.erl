%% EXPECT: maps_iterator_ok
%% Regression: maps:iterator/1 and maps:next/1 are missing, which stops
%% Cowlib 2.22.0 from building. Iteration visits every entry once, ends with
%% none and reports the BEAM errors for a non-map and a non-iterator. Order is
%% unspecified, so entries are compared sorted. Run this fixture on native
%% Nera and BEAM.
-module(maps_iterator).
-export([test/0]).

test() ->
    none = maps:next(maps:iterator(id(#{}))),
    Small = #{a => 1, b => 2, c => 3},
    [{a, 1}, {b, 2}, {c, 3}] = lists:sort(drain(maps:iterator(id(Small)))),
    Large = maps:from_list([{I, I * I} || I <- lists:seq(1, 200)]),
    true = lists:sort(maps:to_list(Large)) =:= lists:sort(drain(maps:iterator(id(Large)))),
    Mixed = #{<<"k">> => [1], {t, 1} => "s", 1.5 => #{x => y}},
    true = lists:sort(maps:to_list(Mixed)) =:= lists:sort(drain(maps:iterator(id(Mixed)))),
    %% Apply and fun references.
    It = apply(maps, iterator, [id(Small)]),
    {_, _, _} = apply(maps, next, [It]),
    Mod = id(maps),
    {_, _, _} = Mod:next((fun maps:iterator/1)(id(Small))),
    %% An iterator is reusable: next/1 does not consume it.
    First = maps:next(It),
    First = maps:next(It),
    %% Errors.
    {badmap, foo} = reason(fun() -> maps:iterator(id(foo)) end),
    {badmap, [{a, 1}]} = reason(fun() -> maps:iterator(id([{a, 1}])) end),
    badarg = reason(fun() -> maps:next(id(foo)) end),
    badarg = reason(fun() -> maps:next(id(1)) end),
    io:format("maps_iterator_ok~n"),
    ok.

drain(It) -> drain(maps:next(It), []).
drain(none, Acc) -> Acc;
drain({K, V, Next}, Acc) -> drain(maps:next(Next), [{K, V} | Acc]).

reason(F) ->
    try F() of
        Value -> {returned, Value}
    catch
        error:Reason -> Reason
    end.

id(X) -> X.
