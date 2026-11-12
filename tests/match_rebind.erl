%% Adapted from OTP compiler/test/match_SUITE.erl (Apache 2.0)
%% Tests sequential rebinding, pre-bound pattern variables, and runtime-
%% observable non-matching aliases (all produce badmatch).
-module(match_rebind).
-export([test/0]).

test() ->
    %% (Pinned-var badmatch on differing value is covered in match_pinned_var.erl.)

    %% Rebinding succeeds when new RHS equals bound value
    X2 = id(1),
    X2 = id(1),
    1 = X2,

    %% Tuple unpack where one element is pre-bound and matches
    XB = id(1),
    {XB, YB} = id({1, 99}),
    99 = YB,

    %% Pattern mentioning pre-bound variable in case head
    ok = pinned_case(3, {tag, 3}),
    other = pinned_case(3, {tag, 4}),

    %% Helper that matches caller-supplied pinned value
    ok = only_if_equal(7, 7),
    nope = only_if_equal(7, 8),

    %% Sequential equivalent of `{ok, V} = {ok, W} = Expr`:
    T2 = id({ok, 42}),
    {ok, VV2} = T2,
    {ok, WW2} = T2,
    42 = VV2,
    42 = WW2,

    %% Runtime non-match via sequential rebinding
    ok = try
             T3 = id({ok, 1}),
             {ok, _VV} = T3,
             {error, _WW} = T3,
             should_not_reach
         catch
             error:{badmatch, {ok, 1}} -> ok
         end,

    io:format("ok~n"),
    ok.

pinned_case(X, T) ->
    case T of
        {tag, X} -> ok;
        _ -> other
    end.

only_if_equal(A, B) ->
    case id(B) of
        A -> ok;
        _ -> nope
    end.

id(I) -> I.
