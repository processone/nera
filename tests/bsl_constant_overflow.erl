%% EXPECT: bsl_constant_overflow ok
-module(bsl_constant_overflow).
-export([test/0]).
test() ->
    A = list_to_integer("9223372036854775808"), A = 1 bsl 63,
    B = list_to_integer("-9223372036854775808"), B = -(1 bsl 63),
    C = list_to_integer("18446744073709551616"), C = 16 bsl 60,
    D = list_to_integer("-18446744073709551616"), D = -16 bsl 60,
    io:format("bsl_constant_overflow ok~n"), ok.
