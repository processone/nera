%% EXPECT: ok
-module(binary_string_codepoints).
-export([test/0]).
test() ->
    <<20>> = <<"🤔">>,
    <<233, 20>> = <<"é🤔">>,
    <<240, 159, 164, 148>> = <<"🤔"/utf8>>,
    io:format("ok~n").
