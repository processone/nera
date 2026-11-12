-module(fun_remote_ref).
-export([test/0, helper/1, adder/2]).

test() ->
    %% Remote fun ref to BIF: erlang:length/1
    F = fun erlang:length/1,
    3 = F([a, b, c]),

    %% Remote fun ref to BIF: erlang:abs/1
    G = fun erlang:abs/1,
    42 = G(id(-42)),

    %% Remote fun ref to BIF: erlang:hd/1
    H = fun erlang:hd/1,
    hello = H([hello, world]),

    %% Remote fun ref to same module
    I = fun fun_remote_ref:helper/1,
    11 = I(10),

    %% Remote fun ref with arity 2
    J = fun fun_remote_ref:adder/2,
    30 = J(id(13), id(17)),

    %% Pass remote fun ref as argument to higher-order function
    L = [1, -2, 3, -4],
    [1, 2, 3, 4] = map(fun erlang:abs/1, L),

    ok.

helper(X) -> X + 1.

adder(A, B) -> A + B.

map(_, []) -> [];
map(F, [H|T]) -> [F(H) | map(F, T)].

id(I) -> I.
