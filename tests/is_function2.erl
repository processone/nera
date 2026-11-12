-module(is_function2).
-export([test/0]).

%% Tests for erlang:is_function/2

test() ->
    F1 = fun(X) -> X end,
    F2 = fun(X, Y) -> X + Y end,
    F0 = fun() -> ok end,

    %% Correct arity
    true = is_function(F1, 1),
    true = is_function(F2, 2),
    true = is_function(F0, 0),

    %% Wrong arity
    false = is_function(F1, 2),
    false = is_function(F2, 1),
    false = is_function(F0, 1),

    %% Non-function
    false = is_function(42, 1),
    false = is_function(hello, 0),
    false = is_function([1,2], 1),

    %% is_function/1 still works
    true = is_function(F1),
    false = is_function(42),

    io:format("ok~n"),
    ok.
