%% Adapted from OTP compiler test suites (Apache 2.0)
%% Tests if expressions.
-module(if_expr).
-export([test/0]).

test() ->
    %% Basic if
    positive = sign(5),
    negative = sign(-3),
    zero = sign(0),

    %% If with multiple conditions
    both = if_multi(true, true),
    first = if_multi(true, false),
    second = if_multi(false, true),
    neither = if_multi(false, false),

    %% If with arithmetic guards
    small = classify(3),
    medium = classify(50),
    large = classify(500),

    %% If with true catch-all
    ok = with_true_guard(anything),

    %% Nested if
    a = nested_if(1, 1),
    b = nested_if(1, 2),
    c = nested_if(2, 1),
    d = nested_if(2, 2),

    io:format("ok~n"),
    ok.

sign(X) ->
    if
        X > 0 -> positive;
        X < 0 -> negative;
        true -> zero
    end.

if_multi(A, B) ->
    if
        A, B -> both;
        A -> first;
        B -> second;
        true -> neither
    end.

classify(X) ->
    if
        X < 10 -> small;
        X < 100 -> medium;
        true -> large
    end.

with_true_guard(_) ->
    if
        true -> ok
    end.

nested_if(X, Y) ->
    if
        X == 1 ->
            if
                Y == 1 -> a;
                true -> b
            end;
        true ->
            if
                Y == 1 -> c;
                true -> d
            end
    end.
