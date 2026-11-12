-module(fun_nested).
-export([test/0]).

test() ->
    %% Nested closures: fun returns a fun
    F = make_multiplier(id(2)),
    G = F(id(3)),
    30 = G(id(5)),

    %% Two-level capture: inner fun captures from both levels
    X = id(10),
    Outer = fun(A) ->
        fun(B) -> A + B + X end
    end,
    Inner = Outer(id(20)),
    35 = Inner(id(5)),

    ok.

make_multiplier(N) ->
    fun(M) ->
        fun(X) -> X * N * M end
    end.

id(I) -> I.
