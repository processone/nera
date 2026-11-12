%% EXPECT: number_separators_ok
%% cow_hpack's upstream 10_000 literal must compile without source rewriting.
-module(number_separators).
-export([test/0]).
-define(SIZE, 10_000).

test() ->
    10000 = ?SIZE,
    255 = 16#F_F,
    5 = 2#1_0_1,
    123456789012345678901 = 123_456_789_012_345_678_901,
    -99999 = -99_999,
    12.34 = 1_2.3_4,
    12340.0 = 1_2.3_4e+0_3,
    0.01234 = 1_2.3_4E-0_3,
    <<255,16,39>> = <<16#f_f, 10_000:16/little>>,
    #{10000 := 255} = #{?SIZE => 16#f_f},
    yes = case id(10000) of 10_000 -> yes; _ -> no end,
    io:format("number_separators_ok~n"),
    ok.
id(X) -> X.
