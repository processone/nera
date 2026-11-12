-module(fun_ref).
-export([test/0]).

test() ->
    %% Local function reference
    F = fun increment/1,
    6 = F(5),

    %% Reference to multi-arity function
    G = fun add/2,
    15 = G(7, 8),

    ok.

increment(X) -> X + 1.

add(A, B) -> A + B.

id(I) -> I.
