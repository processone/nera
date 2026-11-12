-module(fun_simple).
-export([test/0]).

test() ->
    %% Basic anonymous fun, no captures
    F = fun(X) -> X + 1 end,
    6 = F(5),

    %% Fun with multiple args
    Add = fun(A, B) -> A + B end,
    15 = Add(7, 8),

    %% Fun returning a constant
    Const = fun() -> 42 end,
    42 = Const(),

    ok.

id(I) -> I.
