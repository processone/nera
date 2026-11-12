%% Tests for binary:split/2,3 and binary:part/3.
-module(binary_split).
-export([test/0]).

test() ->
    %% binary:split/2 - splits on first occurrence
    [<<"hello">>, <<"world">>] = binary:split(<<"hello world">>, <<" ">>),
    [<<"a">>, <<"b:c">>] = binary:split(<<"a:b:c">>, <<":">>),

    %% No match - returns whole binary
    [<<"hello">>] = binary:split(<<"hello">>, <<"x">>),

    %% Split at beginning
    [<<>>, <<"hello">>] = binary:split(<<":hello">>, <<":">>),

    %% Split at end
    [<<"hello">>, <<>>] = binary:split(<<"hello:">>, <<":">>),

    %% binary:split/3 with [global] - splits on all occurrences
    [<<"a">>, <<"b">>, <<"c">>] = binary:split(<<"a:b:c">>, <<":">>, [global]),
    [<<"one">>, <<"two">>, <<"three">>] = binary:split(<<"one--two--three">>, <<"--">>, [global]),

    %% Global with no match
    [<<"hello">>] = binary:split(<<"hello">>, <<"x">>, [global]),

    %% binary:part/3 - substring extraction
    <<"llo">> = binary:part(<<"hello">>, 2, 3),
    <<"he">> = binary:part(<<"hello">>, 0, 2),
    <<"hello">> = binary:part(<<"hello">>, 0, 5),

    %% part with negative length (count backwards from pos)
    <<"el">> = binary:part(<<"hello">>, 3, -2),

    %% Empty part
    <<>> = binary:part(<<"hello">>, 2, 0),

    io:format("ok~n"),
    ok.
