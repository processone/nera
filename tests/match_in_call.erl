%% Adapted from OTP compiler/test/match_SUITE.erl (Apache 2.0)
%% Tests pattern matching in expression position inside function calls.
-module(match_in_call).
-export([test/0]).

test() ->
    %% (Alias patterns directly inside call arguments are covered in
    %% match_call_alias.erl.) Working variants using a separate binding first:
    X = {gurka, 1},
    {gurka, 1} = id(X),

    %% Using an alias-bound expression as a call argument (top-level binding)
    Y2 = 42,
    42 = id(Y2),

    %% BIF call with pre-bound tuple
    T = {a, b, c},
    a = element(1, T),
    c = element(3, T),

    %% Pre-bound value passed to a constructor call
    P = {ok, 7},
    {wrap, {ok, 7}} = wrap(P),

    %% Pre-bound list passed to a call
    L = [1, 2, 3],
    [1, 2, 3] = first_arg(L, ignored),

    %% Alias in list-comprehension generator element
    Pairs = [{a, 1}, {b, 2}, {c, 3}],
    Keys = [K || {K, _} = _Pair <- Pairs],
    [a, b, c] = Keys,

    %% LC with alias actually using the whole pattern
    Whole = [W || {_, _} = W <- Pairs],
    Pairs = Whole,

    io:format("ok~n"),
    ok.

wrap(V) -> {wrap, V}.

first_arg(A, _B) -> A.

id(I) -> I.
