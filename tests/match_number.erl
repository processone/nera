%% Adapted from OTP compiler/test/match_SUITE.erl (Apache 2.0)
%% Tests number matching edge cases.
-module(match_number).
-export([test/0]).

test() ->
    %% Integer matching
    zero = match_int(0),
    one = match_int(1),
    negative = match_int(-1),
    big = match_int(1000),
    other = match_int(42),

    %% Float matching
    zero_f = match_float(0.0),
    one_f = match_float(1.0),
    other_f = match_float(3.14),

    %% Equality vs exact equality in matching
    %% In pattern matching, 1 does NOT match 1.0
    int = int_or_float(1),
    float = int_or_float(1.0),

    %% Negative number patterns
    neg_one = neg_match(-1),
    neg_two = neg_match(-2),
    other_neg = neg_match(-42),
    not_neg = neg_match(1),

    %% Large numbers
    ok = large_numbers(),

    io:format("ok~n"),
    ok.

match_int(0) -> zero;
match_int(1) -> one;
match_int(-1) -> negative;
match_int(1000) -> big;
match_int(_) -> other.

match_float(0.0) -> zero_f;
match_float(1.0) -> one_f;
match_float(_) -> other_f.

int_or_float(X) when is_integer(X) -> int;
int_or_float(X) when is_float(X) -> float.

neg_match(-1) -> neg_one;
neg_match(-2) -> neg_two;
neg_match(X) when X < 0 -> other_neg;
neg_match(_) -> not_neg.

large_numbers() ->
    X = 1000000,
    Y = 999999,
    1999999 = X + Y,
    1 = X - Y,
    ok.
