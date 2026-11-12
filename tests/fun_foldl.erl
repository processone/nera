-module(fun_foldl).
-export([test/0]).

test() ->
    %% Fold with anonymous fun
    10 = foldl(fun(X, Acc) -> Acc + X end, [1, 2, 3, 4], 0),

    %% Fold with fun ref
    24 = foldl(fun mul/2, [1, 2, 3, 4], 1),

    %% Closure used across multiple recursive calls
    Base = id(1000),
    F = fun(X, Acc) -> Acc + X + Base end,
    4010 = foldl(F, [1, 2, 3, 4], 0),

    %% Reuse same closure multiple times
    Inc = fun(X) -> X + 1 end,
    2 = Inc(1),
    3 = Inc(2),
    11 = Inc(10),

    ok.

foldl(_, [], Acc) -> Acc;
foldl(F, [H|T], Acc) -> foldl(F, T, F(H, Acc)).

mul(A, B) -> A * B.

id(I) -> I.
