-module(preproc_ifdef).
-export([test/0]).

-define(ENABLED, true).

-ifdef(ENABLED).
enabled() -> yes.
-else.
enabled() -> no.
-endif.

-ifndef(DISABLED).
disabled_check() -> correct.
-endif.

-ifdef(DISABLED).
disabled_fn() -> should_not_exist.
-endif.

test() ->
    yes = enabled(),
    correct = disabled_check(),
    ok.
