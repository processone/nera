%% EXPECT_COMPILE_ERROR: variable 'K' is unbound
%% NERA-80: a fun head in an unused record default has the same input scope.
-module(pattern_input_record_unbound).
-export([test/0]).
-record(unused, {value = fun(K, #{K := V}) -> V end}).
test() -> ok.
