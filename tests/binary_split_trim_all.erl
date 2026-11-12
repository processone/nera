%% EXPECT: binary_split_trim_all_ok
-module(binary_split_trim_all).
-export([test/0]).

test() ->
    [<<"a">>, <<"b">>] = binary:split(<<" a  b ">>, <<" ">>, [trim_all, global]),
    [<<"a">>, <<"b">>] = binary:split(<<" a  b ">>, <<" ">>, [global, trim_all]),
    [] = binary:split(<<"   ">>, <<" ">>, [global, trim_all]),
    [] = binary:split(<<>>, <<" ">>, [trim_all]),
    [<<"unchanged">>] = binary:split(<<"unchanged">>, <<" ">>, [global, trim_all]),
    [<<"b c">>] = binary:split(<<" b c">>, <<" ">>, [trim_all]),
    [<<>>, <<"a">>, <<>>, <<>>] = binary:split(<<" a  ">>, <<" ">>, [global]),
    io:format("binary_split_trim_all_ok~n"),
    ok.
