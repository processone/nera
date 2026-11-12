-module(bigint_format).
-export([test/0]).

%% Test: integer_to_list and list_to_integer with bigints
test() ->
    %% Roundtrip a big number through string conversion
    Big = 1 bsl 64,
    S = integer_to_list(Big),
    Big = list_to_integer(S),
    %% Negative bigint
    NegBig = -(1 bsl 64),
    S2 = integer_to_list(NegBig),
    NegBig = list_to_integer(S2),
    %% abs on bigint
    Big = abs(NegBig),
    ok.
