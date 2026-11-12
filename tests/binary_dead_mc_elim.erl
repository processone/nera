-module(binary_dead_mc_elim).
-export([test/0]).

%% Test that byte_size(Rest) works correctly when the match context
%% is dead (eliminated by the dead match context optimization).
%% The IR-level byte_size optimization converts byte_size(MatchCtxVar)
%% into (remaining_bits / 8), so no match context allocation is needed.

test() ->
    Bin = id(<<1,2,3,4,5,6,7,8,9,10>>),
    10 = byte_size(Bin),
    ok = check_rest_sizes(Bin),
    ok = check_loop(Bin, 0),
    ok.

check_rest_sizes(<<_, Rest/binary>>) ->
    9 = byte_size(Rest),
    ok;
check_rest_sizes(<<>>) ->
    ok.

%% Iterative: each step strips one byte, checks remaining size
check_loop(<<>>, 10) -> ok;
check_loop(<<_, Rest/binary>>, N) ->
    Expected = 10 - N - 1,
    Expected = byte_size(Rest),
    check_loop(Rest, N + 1).

id(X) -> X.
