%% Bound binary variables compare their bytes, including deferred tails and
%% sized segments read after a recursive call reuses the match context.
-module(binary_bound_match).
-export([test/0]).

test() ->
    %% Minimal reproducer: the second frame used to compare <<>> with <<1>>.
    2 = copied(<<0,1,1>>, 0),
    Frames = iolist_to_binary([<<N, (binary:copy(<<N>>, N))/binary>>
                              || N <- [0,1,2,64,65,255]]),
    6 = copied(Frames, 0),
    {badmatch, <<1>>} = reason(fun() -> copied(<<0,1,9>>, 0) end),
    function_clause = reason(fun() -> copied(<<0,2,9>>, 0) end),
    0 = tail(<<0>>, <<>>),
    1 = tail(<<0,1>>, <<1>>),
    {badmatch, <<2>>} = reason(fun() -> tail(<<0,1>>, <<2>>) end),
    {badmatch, [1]} = reason(fun() -> tail(<<0,1>>, [1]) end),
    ok = nested(<<0,1,2>>, <<1,2>>),
    {badmatch, {<<9>>, [<<9>>]}} = reason(fun() -> nested(<<0,1,2>>, <<9>>) end),
    first = branch(<<0,1>>, {first, <<1>>}),
    second = branch(<<0,1>>, {second, <<1>>}),
    different = branch(<<0,1>>, {second, <<2>>}),
    ok.

copied(<<>>, Count) -> Count;
copied(<<N, B:N/binary, Rest/binary>>, Count) ->
    B = binary:copy(<<N>>, N),
    copied(Rest, Count + 1).

tail(<<0,B/binary>>, Expected) ->
    B = Expected,
    byte_size(B).

nested(<<0,B/binary>>, Expected) ->
    {B, [B]} = {Expected, [Expected]},
    ok.

branch(<<0,B/binary>>, Expected) ->
    case Expected of
        {first,B} -> first;
        {second,B} -> second;
        _ -> different
    end.

reason(F) ->
    try F() of _ -> unexpected_success catch error:R -> R end.
