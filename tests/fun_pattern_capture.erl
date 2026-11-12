-module(fun_pattern_capture).
-export([test/0]).

test() ->
    %% Capture variables bound in case pattern
    Result1 = case {id(10), id(20)} of
        {A, B} ->
            F = fun(X) -> X + A + B end,
            F(5)
    end,
    35 = Result1,

    %% Capture variable bound via match operator
    {C, D} = {id(100), id(200)},
    G = fun(X) -> X + C + D end,
    303 = G(3),

    %% Capture from nested case
    Result2 = case id(ok) of
        ok ->
            V = id(42),
            case id(1) of
                1 ->
                    H = fun() -> V end,
                    H()
            end
    end,
    42 = Result2,

    ok.

id(I) -> I.
