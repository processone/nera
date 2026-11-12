-module(bigint_factorial).
-export([test/0]).

%% Test: factorial producing huge numbers via bigint promotion
test() ->
    F20 = fac(20),
    2432902008176640000 = F20,
    F50 = fac(50),
    true = F50 > F20,
    true = is_integer(F50),
    %% F50 is a very large number (~65 digits)
    S = integer_to_list(F50),
    true = length(S) > 60,
    ok.

fac(0) -> 1;
fac(N) -> N * fac(N - 1).
