-module(math_utils).
-export([double/1, add/2, sum_list/1, sum_list/2]).

%% Double a number.
double(X) -> X * 2.

%% Add two numbers.
add(X, Y) -> X + Y.

%% Sum a list of numbers.
sum_list(L) -> sum_list(L, 0).

sum_list([], Acc) -> Acc;
sum_list([H|T], Acc) -> sum_list(T, Acc + H).
