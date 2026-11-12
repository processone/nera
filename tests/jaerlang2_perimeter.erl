%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (misc.erl)
%% Tests: tuple pattern matching with different arities, atom tags, float arithmetic
-module(jaerlang2_perimeter).
-export([test/0]).

test() ->
    %% Square
    40 = perimeter({square, 10}),
    4  = perimeter({square, 1}),

    %% Rectangle
    14 = perimeter({rectangle, 3, 4}),
    20 = perimeter({rectangle, 5, 5}),

    %% Circle (approximate pi as 3.14159)
    P = perimeter({circle, 10}),
    true = (P > 62.0),
    true = (P < 63.0),

    %% Triangle
    12 = perimeter({triangle, 3, 4, 5}),
    30 = perimeter({triangle, 10, 10, 10}),

    ok.

perimeter({square, X})            -> 4 * X;
perimeter({rectangle, Width, Ht}) -> 2 * (Width + Ht);
perimeter({circle, R})            -> 2 * 3.14159 * R;
perimeter({triangle, A, B, C})    -> A + B + C.
