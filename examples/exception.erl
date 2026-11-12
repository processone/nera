-module(exception).
-export([start/0]).

start() ->
    %% Test 1: Simple throw/catch
    R1 = try
        throw(hello)
    catch
        Reason -> {caught, Reason}
    end,
    io:format("Test 1: ~w~n", [R1]),

    %% Test 2: No exception (normal return)
    R2 = try
        42
    catch
        _ -> {caught, error}
    end,
    io:format("Test 2: ~w~n", [R2]),

    %% Test 3: Throw with pattern matching in catch
    R3 = try
        throw({error, badarg})
    catch
        {error, What} -> {got_error, What}
    end,
    io:format("Test 3: ~w~n", [R3]),

    ok.
