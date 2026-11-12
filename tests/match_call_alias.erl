%% Adapted from OTP compiler/test/match_SUITE.erl match_in_call/1 (Apache 2.0)
%% Variables introduced via alias patterns inside function-call arguments
%% must be visible in the enclosing scope (Erlang's "exported bindings").
-module(match_call_alias).
-export([test/0]).

test() ->
    %% Alias bound in call argument
    {gurka, 1} = id(X = {gurka, 1}),
    {gurka, 1} = X,

    %% Chained alias in call argument
    42 = id(Y = Z = 42),
    42 = Y,
    42 = Z,

    %% Alias bound via BIF call argument
    a = element(1, T = {a, b, c}),
    {a, b, c} = T,

    io:format("ok~n"),
    ok.

id(I) -> I.
