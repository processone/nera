%% EXPECT: ok
-module(macro_stringify).
-export([test/0]).
-define(S(X), ??X).
-define(V, 42).
-define(PAIR(X), {??X, X}).
test() ->
    "missing ( 255 ) + Value" = ?S(missing(16#ff) + Value),
    "hello" = ?S('hello'),
    "module" = ?S('module'),
    "'case'" = ?S('case'),
    "'hello world'" = ?S('hello world'),
    "\"a\\n\\\"b\\\\c\"" = ?S("a\n\"b\\c"),
    "$a" = ?S($a),
    "$\\n" = ?S($\n),
    "1.0e3" = ?S(1.0e+003),
    "? V" = ?S(?V),
    "<< 1 , 2 >>" = ?S(<<1,2>>),
    "1 + 2" = ?S(1 % discarded comment
                   + 2),
    {"40 + 2", 42} = ?PAIR(40 + 2),
    io:format("ok~n").
