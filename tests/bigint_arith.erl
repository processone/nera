-module(bigint_arith).
-export([test/0]).

%% Test: arithmetic overflow promotes to bigint
test() ->
    %% Small int arithmetic stays small
    3 = 1 + 2,
    0 = 5 - 5,
    6 = 2 * 3,
    %% Overflow promotes to bigint
    Big = 1000000000000000000 * 1000000000000000000,
    true = is_integer(Big),
    %% Bigint comparison
    Threshold = 1000000000000000000 * 999999999,
    true = Big > Threshold,
    %% Bigint + small int
    Big2 = Big + 1,
    true = Big2 > Big,
    %% Bigint - bigint
    1 = Big2 - Big,
    %% Bigint division
    1000000000000000000 = Big div 1000000000000000000,
    %% Bigint remainder
    0 = Big rem 1000000000000000000,
    %% Negate bigint
    NegBig = -Big,
    true = NegBig < 0,
    Big = -NegBig,
    ok.
