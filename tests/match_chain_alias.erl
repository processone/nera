%% Adapted from OTP compiler/test/match_SUITE.erl (Apache 2.0)
%% Alias-chain patterns: `A = B = Expr` at statement level, where every
%% variable in the chain must bind to the matched value.
-module(match_chain_alias).
-export([test/0]).

test() ->
    %% 2-way chain
    A = B = id({1, 2, 3}),
    {1, 2, 3} = A,
    {1, 2, 3} = B,

    %% 3-way chain
    P = Q = R = id({1, 2, 3}),
    {1, 2, 3} = P,
    {1, 2, 3} = Q,
    {1, 2, 3} = R,

    %% Mixed pattern + fresh var chain
    Inner = {inner, YY} = id({inner, 42}),
    {inner, 42} = Inner,
    42 = YY,

    %% Compound chain with two pattern shapes
    {ok, VV} = {ok, WW} = id({ok, 99}),
    99 = VV,
    99 = WW,

    io:format("ok~n"),
    ok.

id(I) -> I.
