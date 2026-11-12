%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (try_test.erl)
%% Tests: try-catch, throw, exit, error, exception classification
-module(jaerlang2_try).
-export([test/0]).

test() ->
    %% Normal return
    {1, normal, a} = catcher(1),

    %% throw
    {2, caught, thrown, a} = catcher(2),

    %% exit
    {3, caught, exited, a} = catcher(3),

    %% Normal value that looks like EXIT tuple
    {4, normal, {'EXIT', a}} = catcher(4),

    %% error
    {5, caught, error, a} = catcher(5),

    ok.

generate_exception(1) -> a;
generate_exception(2) -> throw(a);
generate_exception(3) -> exit(a);
generate_exception(4) -> {'EXIT', a};
generate_exception(5) -> error(a).

catcher(N) ->
    try generate_exception(N) of
        Val -> {N, normal, Val}
    catch
        throw:X -> {N, caught, thrown, X};
        exit:X  -> {N, caught, exited, X};
        error:X -> {N, caught, error, X}
    end.
