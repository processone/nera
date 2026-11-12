-module(preproc_conditionals).
-export([test/0]).

-define(VERSION, 25).
-if(?VERSION >= 25).
-define(SELECTED, modern).
-else.
-define(SELECTED, legacy).
-endif.

-if(false).
-if(?UNDEFINED > 0).
invalid() -> ?ALSO_UNDEFINED.
-endif.
-else.
nested() -> ok.
-endif.

-if((2 < 3) andalso not false).
comparison() -> true.
-else.
comparison() -> false.
-endif.

test() ->
    modern = ?SELECTED,
    true = comparison(),
    ok = nested(),
    ok.
