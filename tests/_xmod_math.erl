-module(xmod_math).
-export([double/1, add/2, sum_list/1]).

%% Helper module for cross-module linking tests.
%% Not a standalone test — compiled together with xmod_basic.erl.

double(X) -> X * 2.

add(X, Y) -> X + Y.

sum_list([]) -> 0;
sum_list([H|T]) -> H + sum_list(T).
