%% Ported from OTP trycatch_SUITE concepts: erlang:raise/3 and rethrow.
-module(catch_raise).
-export([test/0]).

test() ->
    %% Re-raise preserves original stacktrace from 3-arg catch
    ok = reraise_preserves_stack(),
    %% Re-raise with all three classes
    {error, kaboom} = reraise_as(error, kaboom),
    {throw, yikes} = reraise_as(throw, yikes),
    {exit, bye} = reraise_as(exit, bye),
    %% raise/3 with empty stacktrace is a normal exception
    {caught, boom} = raise_empty_stack(),
    %% raise/3 with a synthetic stacktrace carries through
    {caught, boom, [{my_mod, my_fun, 2, _}]} = raise_with_fake_stack(),
    ok.

reraise_preserves_stack() ->
    Stack1 = try
        try
            error(original)
        catch
            C:R:St ->
                erlang:raise(C, R, St)
        end
    catch
        error:original:St2 -> St2
    end,
    true = is_list(Stack1),
    true = length(Stack1) > 0,
    ok.

reraise_as(Class, Reason) ->
    try
        erlang:raise(Class, Reason, [])
    catch
        C:R -> {C, R}
    end.

raise_empty_stack() ->
    try
        erlang:raise(error, boom, [])
    catch
        error:boom -> {caught, boom}
    end.

raise_with_fake_stack() ->
    Fake = [{my_mod, my_fun, 2, []}],
    try
        erlang:raise(error, boom, Fake)
    catch
        error:boom:St -> {caught, boom, St}
    end.
