%% Adapted from OTP compiler/test/match_SUITE.erl (Apache 2.0)
%% Alias patterns (`Pat = Pat`) nested inside tuple/list patterns.
-module(match_nested_alias).
-export([test/0]).

test() ->
    %% Alias inside tuple element
    {outer, Inner = {inner, Y}} = id({outer, {inner, 42}}),
    {inner, 42} = Inner,
    42 = Y,

    %% Alias inside list element
    [First = {ok, V} | _] = id([{ok, 1}, {ok, 2}]),
    {ok, 1} = First,
    1 = V,

    io:format("ok~n"),
    ok.

id(I) -> I.
