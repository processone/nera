-module(fun_closure).
-export([test/0]).

test() ->
    %% Closure capturing one variable
    Y = id(10),
    F = fun(X) -> X + Y end,
    15 = F(5),

    %% Closure capturing multiple variables
    A = id(100),
    B = id(200),
    G = fun(X) -> X + A + B end,
    303 = G(3),

    ok.

id(I) -> I.
