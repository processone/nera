-module(fun_with_lc).
-export([test/0]).

test() ->
    %% Closure called inside a list comprehension
    Base = id(100),
    F = fun(X) -> X + Base end,
    [101, 102, 103] = [F(N) || N <- [1, 2, 3]],

    %% Fun ref used in LC
    [2, 3, 4] = [inc(N) || N <- [1, 2, 3]],

    ok.

inc(X) -> X + 1.

id(I) -> I.
