-module(fun_errors).
-export([test/0]).

test() ->
    %% Calling a non-function should raise badfun
    ok = try
        X = id(42),
        X(5),
        should_not_reach
    catch
        error:{badfun, 42} -> ok
    end,

    %% Wrong arity should raise {badarity, {Fun, Args}}
    ok = try
        F = fun(X) -> X + 1 end,
        F(1, 2),
        should_not_reach
    catch
        error:{badarity, {F, [1, 2]}} when is_function(F, 1) -> ok
    end,

    ok.

id(I) -> I.
