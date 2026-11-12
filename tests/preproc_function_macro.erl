-module(preproc_function_macro).
-export([test/0]).

-define(ADD(X, Y), X + Y).
-define(DOUBLE(X), X * 2).
-define(PAIR(A, B), {A, B}).

test() ->
    5 = ?ADD(2, 3),
    10 = ?DOUBLE(5),
    {hello, world} = ?PAIR(hello, world),
    ok.
