-module(fun_bif_ref).
-export([test/0]).

test() ->
    F = fun abs/1,
    1 = F(-1),
    5 = F(5),

    G = fun length/1,
    3 = G([a, b, c]),

    H = fun hd/1,
    a = H([a, b]),

    %% Used as higher-order function argument
    [1, 2, 3] = lists:map(fun abs/1, [-1, -2, -3]),

    ok.
