-module(dynamic_fun_ref).
-export([test/0, helper/1, adder/2]).

test() ->
    %% All-variable: fun M:F/A
    M = erlang,
    F = length,
    A = 1,
    Ref1 = fun M:F/A,
    3 = Ref1([a, b, c]),

    %% Variable module, atom function, integer arity
    M2 = erlang,
    Ref2 = fun M2:abs/1,
    42 = Ref2(-42),

    %% Atom module, variable function
    F2 = hd,
    Ref3 = fun erlang:F2/1,
    hello = Ref3([hello, world]),

    %% Atom module, atom function, variable arity
    A2 = 1,
    Ref4 = fun erlang:length/A2,
    0 = Ref4([]),

    %% Dynamic ref to same module function
    Mod = dynamic_fun_ref,
    Fun = helper,
    Ref5 = fun Mod:Fun/1,
    11 = Ref5(10),

    %% Dynamic ref with arity 2
    Ref6 = fun Mod:adder/2,
    30 = Ref6(13, 17),

    %% Pass dynamic fun ref to higher-order function
    L = [1, -2, 3, -4],
    AbsFun = abs,
    [1, 2, 3, 4] = map(fun erlang:AbsFun/1, L),

    ok.

helper(X) -> X + 1.

adder(A, B) -> A + B.

map(_, []) -> [];
map(F, [H|T]) -> [F(H) | map(F, T)].
