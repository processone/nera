%% Tests: nested try/catch, exception in after, throw vs error vs exit.
-module(nested_try_catch).
-export([test/0]).

test() ->
    %% Basic throw/error/exit distinction
    {caught_throw, oops} = catch_kind_throw(fun() -> throw(oops) end),
    {caught_error, boom} = catch_kind_error(fun() -> error(boom) end),
    {caught_exit, bye} = catch_kind_exit(fun() -> exit(bye) end),

    %% Nested try: inner catches, outer doesn't see it
    {inner, caught} = nested_inner_catches(),

    %% Try with after block
    {ok, after_ran} = try_with_after(),

    %% Exception in after block — after exception propagates
    after_exception = (catch try_after_throws()),

    %% Re-raise pattern: catch, inspect, re-throw
    re_raised = (catch reraise_test()),

    %% Try-of with pattern matching on success value
    {matched, 42} = try_of_match(42),
    no_match = try_of_match(99),

    %% Catch expression (old-style)
    foo = old_catch_test(),

    %% Double try: inner error + outer error
    double_ok = double_try(),

    %% Try with multiple catch clauses
    got_error = multi_catch(fun() -> error(x) end),
    got_throw = multi_catch(fun() -> throw(x) end),

    io:format("ok~n"),
    ok.

catch_kind_throw(Fun) ->
    try
        Fun()
    catch
        throw:Reason -> {caught_throw, Reason}
    end.

catch_kind_error(Fun) ->
    try
        Fun()
    catch
        error:Reason -> {caught_error, Reason}
    end.

catch_kind_exit(Fun) ->
    try
        Fun()
    catch
        exit:Reason -> {caught_exit, Reason}
    end.

nested_inner_catches() ->
    try
        try
            error(inner_problem)
        catch
            error:inner_problem -> {inner, caught}
        end
    catch
        _:_ -> {outer, caught}
    end.

try_with_after() ->
    Self = self(),
    try
        42
    after
        Self ! after_ran
    end,
    receive
        after_ran -> {ok, after_ran}
    after 100 ->
        error(no_after)
    end.

try_after_throws() ->
    try
        ok
    after
        throw(after_exception)
    end.

reraise_test() ->
    try
        try
            error(original)
        catch
            error:original ->
                throw(re_raised)
        end
    catch
        throw:re_raised -> throw(re_raised)
    end.

try_of_match(X) ->
    try id(X) of
        42 -> {matched, 42};
        _ -> no_match
    catch
        _:_ -> error_result
    end.

old_catch_test() ->
    Result = catch throw(foo),
    Result.

double_try() ->
    Outer = try
        try
            error(first)
        catch
            error:first -> got_first
        end
    catch
        _:_ -> outer_fail
    end,
    case Outer of
        got_first -> double_ok;
        _ -> wrong
    end.

multi_catch(Fun) ->
    try
        Fun()
    catch
        error:_ -> got_error;
        throw:_ -> got_throw;
        exit:_ -> got_exit
    end.

id(X) -> X.
