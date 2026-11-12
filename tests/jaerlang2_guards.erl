%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (misc.erl)
%% Tests: guard expressions (is_integer, >, comparisons), multiple clauses
-module(jaerlang2_guards).
-export([test/0]).

test() ->
    %% perimeter with guards
    40 = perimeter({square, 10}),
    26 = perimeter({rectangle, 5, 8}),
    12 = perimeter({triangle, 3, 4, 5}),

    %% factorial with guards
    1       = fac(0),
    120     = fac(5),
    3628800 = fac(10),

    %% max/min with guards
    5 = max(3, 5),
    5 = max(5, 3),
    3 = min(3, 5),
    3 = min(5, 3),

    %% abs
    5 = myabs(5),
    5 = myabs(-5),
    0 = myabs(0),

    %% classify_age
    child  = classify_age(10),
    teen   = classify_age(15),
    adult  = classify_age(25),
    senior = classify_age(70),

    ok.

perimeter({square, X})            -> 4 * X;
perimeter({rectangle, Width, Ht}) -> 2 * (Width + Ht);
perimeter({triangle, A, B, C})    -> A + B + C.

fac(0) -> 1;
fac(N) when N > 0 -> N * fac(N-1).

max(X, Y) when X >= Y -> X;
max(_, Y) -> Y.

min(X, Y) when X =< Y -> X;
min(_, Y) -> Y.

myabs(X) when X < 0 -> -X;
myabs(X) -> X.

classify_age(Age) when Age < 13 -> child;
classify_age(Age) when Age < 18 -> teen;
classify_age(Age) when Age < 65 -> adult;
classify_age(_) -> senior.
