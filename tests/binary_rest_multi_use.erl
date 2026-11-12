-module(binary_rest_multi_use).
-export([test/0]).

%% Regression test: when a rest segment variable is used both as a
%% function call argument (match context advance) AND in another
%% expression (cons, tuple, return), the variable must be properly
%% materialized for the non-call use.

test() ->
    %% Rest used in call AND in cons cell
    [<<2,3>>, done] = call_and_cons(<<1,2,3>>),

    %% Rest used in call AND in tuple
    {<<3,4,5>>, 3} = call_and_tuple(<<1,2,3,4,5>>),

    %% Rest used in call AND returned directly
    {<<4,5>>, <<4,5>>} = call_and_return(<<1,2,3,4,5>>),

    %% Multiple bytes extracted, rest used in call and cons
    [<<5>>, parsed] = multi_byte_and_cons(<<1,2,3,4,5>>),

    ok.

call_and_cons(<<_, Rest/binary>>) ->
    Result = count_bytes(Rest, 0),
    case Result of
        _ -> [Rest, done]
    end.

call_and_tuple(<<_, _, Rest/binary>>) ->
    N = count_bytes(Rest, 0),
    {Rest, N}.

call_and_return(<<_, _, _, Rest/binary>>) ->
    Copy = identity(Rest),
    {Rest, Copy}.

multi_byte_and_cons(<<_, _, _, _, Rest/binary>>) ->
    _ = count_bytes(Rest, 0),
    [Rest, parsed].

count_bytes(<<>>, Acc) -> Acc;
count_bytes(<<_, Rest/binary>>, Acc) ->
    count_bytes(Rest, Acc + 1).

identity(X) -> X.
