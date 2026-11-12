-module(catch_stacktrace).
-export([test/0]).

test() ->
    %% 3-part catch: Class:Reason:Stacktrace
    {error, boom, Stack} = get_stack(),
    true = is_list(Stack),

    %% Stacktrace should contain at least one frame
    true = length(Stack) > 0,

    %% Each frame should be a 4-tuple {Module, Function, Arity, Info}
    {_, _, _, _} = hd(Stack),

    %% 2-part catch still works
    boom = get_reason(),

    %% Wildcard stacktrace pattern
    caught_wild = wildcard_stack(),

    %% Mixed clauses: some with stacktrace, some without
    {throw, oops} = mixed_clauses_throw(),
    {error, fail, true} = mixed_clauses_error(),

    %% Different exception classes with stacktrace
    {throw, yikes, true} = throw_with_stack(),
    {exit, bye, true} = exit_with_stack(),

    %% Variable class with stacktrace
    {error, kaboom, true} = var_class_stack(),

    ok.

get_stack() ->
    try
        error(boom)
    catch
        Class:Reason:Stacktrace -> {Class, Reason, Stacktrace}
    end.

get_reason() ->
    try
        error(boom)
    catch
        error:Reason -> Reason
    end.

wildcard_stack() ->
    try
        error(oops)
    catch
        _:_:_ -> caught_wild
    end.

mixed_clauses_throw() ->
    try
        throw(oops)
    catch
        throw:R -> {throw, R};
        error:R:St -> {error, R, St}
    end.

mixed_clauses_error() ->
    try
        error(fail)
    catch
        throw:R -> {throw, R};
        error:R:St -> {error, R, is_list(St)}
    end.

throw_with_stack() ->
    try
        throw(yikes)
    catch
        throw:R:St -> {throw, R, is_list(St)}
    end.

exit_with_stack() ->
    try
        exit(bye)
    catch
        exit:R:St -> {exit, R, is_list(St)}
    end.

var_class_stack() ->
    try
        error(kaboom)
    catch
        C:R:St -> {C, R, is_list(St)}
    end.
