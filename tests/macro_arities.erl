%% EXPECT: ok
-module(macro_arities).
-export([test/0]).
-define(VALUE(), 7).
-define(VALUE(X), X).
-define(VALUE(X, Y), {X, Y}).
-define(REMOVED(X), X).
-define(REMOVED(X, Y), {X, Y}).
-undef(REMOVED).
-ifdef(REMOVED).
-error(undef_must_remove_every_arity).
-endif.
test() ->
    7 = ?VALUE(),
    42 = ?VALUE(42),
    {1, 2} = ?VALUE(1, 2),
    io:format("ok~n").
