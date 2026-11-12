%% Pre-existing, separate sub-byte cursor bug discovered during extended checks.
%% BEAM prints 3; baseline Nera prints 4. Kept outside the passing regression suite.
-module(bit_offset_probe).
-export([start/0]).
start() -> io:format("~p~n", [bits(<<1:1,0:1,1:1,1:1>>, 0)]), halt(0).
bits(<<>>, A) -> A;
bits(<<C:1, Rest/bitstring>>, A) -> bits(Rest, A+C).
