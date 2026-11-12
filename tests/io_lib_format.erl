%% Tests for io_lib:format/2.
-module(io_lib_format).
-export([test/0]).

test() ->
    %% Basic string formatting
    "hello" = lists:flatten(io_lib:format("~s", ["hello"])),

    %% Integer formatting
    "42" = lists:flatten(io_lib:format("~p", [42])),
    "42" = lists:flatten(io_lib:format("~w", [42])),

    %% Atom formatting
    "ok" = lists:flatten(io_lib:format("~p", [ok])),
    "hello" = lists:flatten(io_lib:format("~p", [hello])),

    %% Newline - io_lib:format("~n", []) returns [10]
    [10] = lists:flatten(io_lib:format("~n", [])),

    %% Mixed format
    "Port 8080" = lists:flatten(io_lib:format("Port ~p", [8080])),

    %% Tilde escape
    "~" = lists:flatten(io_lib:format("~~", [])),

    %% Multiple arguments
    "a and b" = lists:flatten(io_lib:format("~p and ~p", [a, b])),

    %% String with ~s
    "key=value" = lists:flatten(io_lib:format("~s=~s", ["key", "value"])),

    io:format("ok~n"),
    ok.
