%% Adapted from OTP compiler test suites (Apache 2.0)
%% Tests string handling (strings are lists of integers in Erlang).
-module(string_as_list).
-export([test/0]).

test() ->
    %% Strings are lists
    true = is_list("hello"),
    5 = length("hello"),

    %% Character values
    104 = hd("hello"),

    %% String comparison
    true = "abc" == "abc",
    true = "abc" < "abd",
    true = "abc" < "abcd",
    false = "abc" == "abd",

    %% String concatenation (list append)
    "helloworld" = "hello" ++ "world",
    "abc" = "a" ++ "bc",

    %% Empty string
    true = "" == [],
    0 = length(""),
    "hello" = "" ++ "hello",
    "hello" = "hello" ++ "",

    %% String as list of integers
    [104, 101, 108, 108, 111] = "hello",

    %% Pattern matching on strings
    greeting = classify_string("hello"),
    farewell = classify_string("bye"),
    other = classify_string("xyz"),

    io:format("ok~n"),
    ok.

classify_string("hello") -> greeting;
classify_string("bye") -> farewell;
classify_string(_) -> other.
