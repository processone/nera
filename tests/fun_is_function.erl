-module(fun_is_function).
-export([test/0]).

test() ->
    F = fun(X) -> X + 1 end,

    %% is_function returns true for funs
    true = erlang:is_function(F),

    %% is_function returns false for non-funs
    false = erlang:is_function(42),
    false = erlang:is_function(hello),
    false = erlang:is_function([1, 2]),
    false = erlang:is_function({a, b}),

    %% fun ref is also a function
    G = fun id/1,
    true = erlang:is_function(G),

    %% is_function in guard
    ok = check_fun(F),
    ok = check_fun(G),
    not_fun = check_fun(42),

    ok.

check_fun(F) when is_function(F) -> ok;
check_fun(_) -> not_fun.

id(I) -> I.
