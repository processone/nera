%% Adapted from OTP compiler test suites (Apache 2.0)
%% Tests various control flow: case, begin...end, nested expressions.
-module(control_flow).
-export([test/0]).

test() ->
    %% begin...end blocks
    3 = begin 1, 2, 3 end,
    ok = begin
        X = 1,
        Y = 2,
        3 = X + Y,
        ok
    end,

    %% Nested case expressions
    {big, positive} = classify(100),
    {small, positive} = classify(3),
    {small, negative} = classify(-3),
    {big, negative} = classify(-100),
    zero = classify(0),

    %% Case in function arguments
    6 = double(case id(1) + id(2) of V -> V end),

    %% Nested case with guards
    pos_even = categorize(4),
    pos_odd = categorize(3),
    neg = categorize(-5),
    zero = categorize(0),

    %% Complex control flow
    [6, 3] = process_list([1, 2, 3]),

    %% Sequential expressions
    ok = sequential(),

    io:format("ok~n"),
    ok.

classify(0) -> zero;
classify(X) ->
    Size = case X of
        N when N > 10; N < -10 -> big;
        _ -> small
    end,
    Sign = case X of
        P when P > 0 -> positive;
        _ -> negative
    end,
    {Size, Sign}.

double(X) -> X * 2.

categorize(0) -> zero;
categorize(X) when X > 0, X rem 2 == 0 -> pos_even;
categorize(X) when X > 0 -> pos_odd;
categorize(_) -> neg.

process_list(L) ->
    Sum = sum(L),
    Len = length(L),
    [Sum, Len].

sum([]) -> 0;
sum([H | T]) -> H + sum(T).

sequential() ->
    A = 1,
    B = A + 1,
    C = B + 1,
    6 = A + B + C,
    ok.

id(I) -> I.
