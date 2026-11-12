-module(preproc_define).
-export([test/0]).

-define(ANSWER, 42).
-define(HELLO, "Hello").
-define(ATUPLE, {a, b, c}).

test() ->
    42 = ?ANSWER,
    "Hello" = ?HELLO,
    {a, b, c} = ?ATUPLE,
    ok.
