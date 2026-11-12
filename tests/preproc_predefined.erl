-module(preproc_predefined).
-export([test/0]).

test() ->
    preproc_predefined = ?MODULE,
    true = is_integer(?LINE),
    true = ?LINE > 0,
    ok.
