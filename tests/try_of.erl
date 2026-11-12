-module(try_of).
-export([test/0]).

%% Tests for try...of...catch...end syntax.

test() ->
    %% 1. Basic try...of with matching clause
    {ok, 42} = try_of_basic(42),
    {ok, hello} = try_of_basic(hello),

    %% 2. try...of with multiple clauses
    pos = try_of_sign(5),
    neg = try_of_sign(-3),
    zero = try_of_sign(0),

    %% 3. try...of with exception in body
    caught = try_of_with_throw(),

    %% 4. try...of...catch...after
    {ok, 10} = try_of_after(10),

    io:format("ok~n"),
    ok.

try_of_basic(V) ->
    try
        id(V)
    of
        X -> {ok, X}
    catch
        _:_ -> error
    end.

try_of_sign(N) ->
    try
        id(N)
    of
        X when X > 0 -> pos;
        X when X < 0 -> neg;
        0 -> zero
    catch
        _:_ -> error
    end.

try_of_with_throw() ->
    try
        throw(oops)
    of
        _ -> should_not_reach
    catch
        throw:oops -> caught
    end.

try_of_after(V) ->
    try
        id(V)
    of
        X -> {ok, X}
    catch
        _:_ -> error
    after
        ok
    end.

id(X) -> X.
