%% Tests io:format with supported directives.
%% EXPECT: 42|hello|[1,2,3]|{a,b}|done
-module(io_format).
-export([test/0]).

test() ->
    %% ~w writes any term
    io:format("~w", [42]),
    io:format("|"),
    io:format("~w", [hello]),
    io:format("|"),
    io:format("~w", [[1, 2, 3]]),
    io:format("|"),
    io:format("~w", [{a, b}]),
    io:format("|"),

    %% ~s writes a string (list of chars)
    io:format("~s", ["done"]),
    io:format("~n"),

    ok.
