%% Adapted from OTP erts/emulator/test/exception_SUITE.erl (Apache 2.0)
%% Tests basic exception handling with try/catch.
-module(exception_basic).
-export([test/0]).

test() ->
    %% Catch a throw
    {caught, hello} = try_throw(hello),
    {caught, 42} = try_throw(42),

    %% Catch an error
    {error_caught, badarg} = try_error(badarg),
    {error_caught, badarith} = try_error(badarith),

    %% Catch an exit
    {exit_caught, normal} = try_exit(normal),
    {exit_caught, reason} = try_exit(reason),

    %% Try with no exception
    {ok, 42} = try_normal(42),
    {ok, hello} = try_normal(hello),

    %% Try with pattern matching in catch
    {matched, badarg} = try_match(badarg),
    {other, something} = try_match(something),

    %% Nested try
    {inner, hello} = nested_try(hello),

    %% Try in tail position
    caught = try_tail(true),
    ok = try_tail(false),

    io:format("ok~n"),
    ok.

try_throw(V) ->
    try
        throw(V)
    catch
        throw:X -> {caught, X}
    end.

try_error(V) ->
    try
        error(V)
    catch
        error:X -> {error_caught, X}
    end.

try_exit(V) ->
    try
        exit(V)
    catch
        exit:X -> {exit_caught, X}
    end.

try_normal(V) ->
    try
        V
    catch
        _:_ -> error
    end,
    {ok, V}.

try_match(V) ->
    try
        error(V)
    catch
        error:badarg -> {matched, badarg};
        error:X -> {other, X}
    end.

nested_try(V) ->
    try
        try
            throw(V)
        catch
            throw:X -> {inner, X}
        end
    catch
        _:_ -> outer
    end.

try_tail(true) ->
    try
        throw(oops)
    catch
        throw:_ -> caught
    end;
try_tail(false) ->
    ok.
