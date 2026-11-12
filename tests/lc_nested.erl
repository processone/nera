-module(lc_nested).
-export([test/0]).

%% Tests for nested list comprehension generators.

test() ->
    %% 1. Two generators — cartesian product
    L1 = [1, 2],
    L2 = [a, b],
    [{1,a},{1,b},{2,a},{2,b}] = [{X,Y} || X <- L1, Y <- L2],

    %% 2. Three generators
    R = [{X,Y,Z} || X <- [1], Y <- [a,b], Z <- [x,y]],
    [{1,a,x},{1,a,y},{1,b,x},{1,b,y}] = R,

    %% 3. Nested with filter
    Evens = [{X,Y} || X <- [1,2,3], Y <- [1,2,3], (X + Y) rem 2 =:= 0],
    [{1,1},{1,3},{2,2},{3,1},{3,3}] = Evens,

    %% 4. Single generator still works
    [2,4,6] = [X * 2 || X <- [1,2,3]],

    io:format("ok~n"),
    ok.
