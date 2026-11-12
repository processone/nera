-module(fun_multi_clause).
-export([test/0]).

test() ->
    %% Multi-clause fun with guards
    Sign = fun(X) when X > 0 -> positive;
              (X) when X < 0 -> negative;
              (0) -> zero
           end,
    positive = Sign(id(5)),
    negative = Sign(id(-3)),
    zero = Sign(id(0)),

    ok.

id(I) -> I.
