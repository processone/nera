%% EXPECT: max_min_numbers_ok
%% Regression: erlang:max/2 and min/2 return one of their two arguments
%% unchanged, the first when they compare equal, as on BEAM. Natively a mixed
%% integer and float pair turned the integer into a new float, so max(2, 1.5)
%% returned 2.0 and max(1, 1.0) returned 1.0, and integers above 2^53 lost
%% precision before comparing. A match is exact, so 2 = 2.0 fails. Arguments
%% pass through id/1 so that neither compiler folds the calls. Run this same
%% fixture on native Nera and BEAM.
-module(max_min_numbers).
-export([test/0]).

test() ->
    %% The larger or smaller argument comes back with its own type.
    2 = max(id(2), id(1.5)),
    2 = max(id(1.5), id(2)),
    1.5 = min(id(2), id(1.5)),
    1 = min(id(2.5), id(1)),
    2.5 = max(id(1), id(2.5)),
    -2.5 = max(id(-3), id(-2.5)),
    -3 = min(id(-2.5), id(-3)),

    %% An integer and a float that compare equal: the first one comes back.
    1 = max(id(1), id(1.0)),
    1.0 = max(id(1.0), id(1)),
    1 = min(id(1), id(1.0)),
    1.0 = min(id(1.0), id(1)),
    -3 = max(id(-3), id(-3.0)),
    -3.0 = min(id(-3.0), id(-3)),
    0 = max(id(0), id(-0.0)),
    0 = min(id(0), id(0.0)),
    1 = sign(max(id(-0.0), id(0))),
    0 = sign(min(id(0.0), id(0))),

    %% Integers compare with floats exactly, without rounding to a float.
    %% 2^53 + 1 is larger than the float 2^53, which it would round to.
    9007199254740993 = max(id(9007199254740993), id(9007199254740992.0)),
    9007199254740993 = max(id(9007199254740992.0), id(9007199254740993)),
    9007199254740992.0 = min(id(9007199254740993), id(9007199254740992.0)),
    -9007199254740993 = min(id(-9.007199254740992e15), id(-9007199254740993)),
    9007199254740992 = max(id(9007199254740992), id(9007199254740992.0)),
    9007199254740992.0 = max(id(9007199254740992.0), id(9007199254740992)),
    %% 2^60 - 1 rounds to the float 2^60 but is smaller than it; 2^60 equals it.
    1152921504606846975 = min(id(1152921504606846975), id(1152921504606846976.0)),
    1152921504606846976.0 = max(id(1152921504606846975), id(1152921504606846976.0)),
    1152921504606846976 = max(id(1152921504606846976), id(1152921504606846976.0)),
    1152921504606846976.0 = min(id(1152921504606846976.0), id(1152921504606846976)),
    %% 2^63 - 1 is smaller than the float 2^63.
    9223372036854775807 = min(id(9223372036854775807), id(9223372036854775808.0)),
    9223372036854775808.0 = max(id(9223372036854775808.0), id(9223372036854775807)),
    %% Integers beyond 64 bits.
    Big = 1 bsl 70,
    BigF = float(Big),
    Big = max(id(Big), id(BigF)),
    BigF = max(id(BigF), id(Big)),
    Big1 = Big + 1,
    Big1 = max(id(BigF), id(Big1)),
    BigF = min(id(Big1), id(BigF)),

    %% Two integers or two floats keep their order and the first when equal.
    7 = max(id(3), id(7)),
    -7 = min(id(-3), id(-7)),
    2.5 = max(id(1.5), id(2.5)),
    0 = sign(max(id(0.0), id(-0.0))),
    1 = sign(max(id(-0.0), id(0.0))),
    1 = sign(min(id(-0.0), id(0.0))),

    %% Every way of calling them gives the same answers.
    2 = apply(erlang, max, [id(2), id(1.5)]),
    1 = apply(erlang, min, [id(1), id(1.0)]),
    Max = fun erlang:max/2,
    Min = fun erlang:min/2,
    2 = Max(id(2), id(1.5)),
    1 = Min(id(1), id(1.0)),
    first = guard(id(2), id(1.5)),
    first = guard(id(1), id(1.0)),
    second = guard(id(1.5), id(2)),
    io:format("max_min_numbers_ok~n"),
    ok.

guard(X, Y) when max(X, Y) =:= X -> first;
guard(_, _) -> second.

%% The sign bit of a float: 1 for -0.0, which compares equal to 0.0.
sign(X) when is_float(X) ->
    <<S:1, _:63>> = <<X:64/float>>,
    S.

id(X) -> X.
