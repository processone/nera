%% EXPECT: macro_block_arguments_ok
-module(macro_block_arguments).
-export([test/0]).
-define(PAIR(A, B), {A, B}).
-define(FORALL(Variable, Generator, Property), fun(Variable) -> Property end).

test() ->
    {2, 3} = ?PAIR(begin X = 1, X + 1 end, 3),
    F = ?FORALL(V, ignored, begin Y = V + 1, Y =:= 2 end),
    true = F(1),
    {4, 9} = ?PAIR(begin Z = case 1 of 1 -> W = 3, W + 1 end, Z end, 9),
    {Ref, 7} = ?PAIR(fun erlang:abs/1, 7),
    3 = Ref(-3),
    {Anon, 8} = ?PAIR(fun(A) -> B = A + 1, B end, 8),
    2 = Anon(1),
    {Named, 9} = ?PAIR(fun Loop(0) -> 0; Loop(N) -> Next = N - 1, Loop(Next) end, 9),
    0 = Named(2),
    io:format("macro_block_arguments_ok~n"),
    ok.
