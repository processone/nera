-module(char_literal).
-export([test/0]).

%% Tests for character literals ($a, $\n, etc.)

test() ->
    %% Plain character literals
    97 = $a,
    65 = $A,
    48 = $0,
    64 = $@,
    95 = $_,

    %% Escape sequences
    10 = $\n,
    9 = $\t,
    13 = $\r,
    92 = $\\,
    32 = $\s,

    %% Arithmetic with character literals
    true = $a =:= 97,
    true = $z =:= 122,
    true = ($z - $a) =:= 25,

    %% Character literal in pattern matching
    97 = id($a),
    10 = id($\n),

    %% Use in list (string-like)
    [104, 105] = [$h, $i],

    io:format("ok~n"),
    ok.

id(X) -> X.
