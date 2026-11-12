%% Adapted from OTP compiler/test/match_SUITE.erl (Apache 2.0)
%% Tests alias-chain patterns: A = B = C, string/list alias equivalence,
%% character-literal aliases, multi-variable tuple aliases, nested aliases.
-module(match_alias_chain).
-export([test/0]).

test() ->
    %% (Top-level `A = B = Expr` chains covered in match_chain_alias.erl.)

    %% String vs list alias equivalence in function head
    "abc" = id_string("abc"),
    "abc" = id_string([$a, $b, $c]),
    S = id_string("abc"),
    "abc" = S,

    %% Character-literal aliases
    $v = 118,

    %% Multi-variable tuple alias
    {X1, Y1} = id({1, 2}),
    {X2, Y2} = {X1, Y1},
    1 = X1, 2 = Y1,
    1 = X2, 2 = Y2,

    %% Cons alias: pattern = variable, sequential
    L = id([1, 2, 3]),
    [H | T] = L,
    1 = H,
    [2, 3] = T,
    [1, 2, 3] = L,

    %% (Nested alias like {outer, X = {inner, Y}} = ... covered in
    %% match_nested_alias.erl.) Sequential variant:
    Inner = id({inner, 42}),
    {inner, YY} = Inner,
    42 = YY,

    %% Alias in function head
    {whole, {ok, 7}, 7} = head_alias({ok, 7}),
    {whole, {error, bad}, bad} = head_alias({error, bad}),

    %% Alias in function head with guard
    ok = head_alias_guard({tag, 5}),
    fail = head_alias_guard({tag, -1}),

    S0 = "abc",
    S0 = id_string(S0),

    io:format("ok~n"),
    ok.

head_alias({ok, V} = W) -> {whole, W, V};
head_alias({error, Reason} = W) -> {whole, W, Reason}.

head_alias_guard({tag, V} = _W) when V > 0 -> ok;
head_alias_guard(_) -> fail.

id_string([$a, $b, $c] = Str) -> Str.

id(I) -> I.
