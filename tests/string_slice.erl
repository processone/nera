%% EXPECT: ok
%% string:slice/3 over graphemes as on OTP 29. Expected values were taken
%% from BEAM. Lists and mixed chardata are decoded up front, so results for
%% those are compared as flat chardata.
-module(string_slice).
-export([test/0]).
test() ->
    <<"ell">> = string:slice(<<"hello">>, 1, 3),
    "ell" = string:slice("hello", 1, 3),
    "ello" = string:slice("hello", 1, infinity),
    <<"ello">> = string:slice(<<"hello">>, 1, infinity),
    [] = string:slice("hello", 10, 3),
    <<>> = string:slice(<<"hello">>, 9, 2),
    [] = string:slice("hello", 1, 0),
    <<>> = string:slice(<<"hello">>, 1, 0),
    [] = string:slice(atom, -1, 0),
    <<>> = string:slice(<<>>, 0, 3),
    [] = string:slice([], 3, 3),
    "ell" = string:slice([<<"he">>,"llo"], 1, 3),
    "ello" = string:slice([<<"he">>,"llo"], 1, 30),
    "l" = string:slice([<<"he">>,"llo"], 2, 1),
    "ab" = string:slice([<<"a">>,<<"b">>], 0, 2),
    "bcd" = string:slice(["ab","cd"], 1, infinity),
    <<"bc">> = string:slice(<<"abcd">>, 1, 2),
    %% Graphemes, not codepoints.
    <<97,204,129>> = string:slice(<<"a",769/utf8,"bc">>, 0, 1),
    <<"b">> = string:slice(<<"a",769/utf8,"bc">>, 1, 1),
    [97,769] = string:slice([97,769,98], 0, 1),
    "\r\n" = string:slice("a\r\nb", 1, 1),
    <<"é"/utf8>> = string:slice(<<"éé"/utf8>>, 1, 1),
    %% Large counts.
    "ab" = string:slice("ab", 0, 100000000000000000000),
    [] = string:slice("abc", 100000000000000000000, 3),
    %% The input is returned as given when nothing is skipped.
    S = [<<"a">>,<<"b">>],
    true = S =:= string:slice(S, 0, infinity),
    %% Invalid UTF-8 only fails once the scan reaches it.
    <<"a">> = string:slice(<<"ab",255>>, 0, 1),
    <<"bc",255>> = string:slice(<<"abc",255>>, 1, infinity),
    <<255>> = string:slice(<<"abc",255>>, 3, infinity),
    <<"é"/utf8>> = string:slice(<<"éé"/utf8,255>>, 0, 1),
    {badarg, <<255>>} = failure(fun() -> string:slice(<<255>>, 0, 1) end),
    {badarg, <<255>>} = failure(fun() -> string:slice(<<"a",255>>, 0, 1) end),
    {badarg, <<255>>} = failure(fun() -> string:slice(<<"ab",255>>, 0, 5) end),
    {badarg, <<255>>} = failure(fun() -> string:slice(<<"ab",255>>, 5, 5) end),
    {badarg, <<255>>} = failure(fun() -> string:slice(<<"abc",255>>, 4, infinity) end),
    function_clause = failure(fun() -> string:slice(<<"hello">>, -1, 1) end),
    function_clause = failure(fun() -> string:slice(<<"hello">>, 1, -1) end),
    function_clause = failure(fun() -> string:slice(<<"hello">>, a, 1) end),
    function_clause = failure(fun() -> string:slice(atom, 0, 1) end),
    function_clause = failure(fun() -> string:slice([$a,bad], 0, 1) end),
    function_clause = failure(fun() -> string:slice("abc", 0, 5.0) end),
    function_clause = failure(fun() -> string:slice("abc", 0, inf) end),
    <<"el">> = apply(string, slice, [<<"hello">>, 1, 2]),
    F = fun string:slice/3,
    "el" = F("hello", 1, 2),
    io:format("ok~n").
failure(F) -> try F() of _ -> unexpected_return catch error:R -> R end.
