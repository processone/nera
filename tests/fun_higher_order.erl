-module(fun_higher_order).
-export([test/0]).

test() ->
    %% Passing a fun as argument
    L = [1, 2, 3],
    [2, 3, 4] = map(fun(X) -> X + 1 end, L),

    %% Using a fun ref as argument
    [2, 4, 6] = map(fun double/1, L),

    %% Returning a fun from a function
    Adder = make_adder(10),
    15 = Adder(5),

    ok.

map(F, [H|T]) -> [F(H) | map(F, T)];
map(_, []) -> [].

double(X) -> X * 2.

make_adder(N) ->
    fun(X) -> X + N end.

id(I) -> I.
