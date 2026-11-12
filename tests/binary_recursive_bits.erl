%% EXPECT: ok
-module(binary_recursive_bits).
-export([test/0]).

test() ->
    3 = sum(<<1:1,0:1,1:1,1:1>>, 0),
    %% Exhaust all byte values and all starting positions, crossing byte boundaries.
    check(0),
    %% A refcounted backing binary and enough recursive steps to cross safepoints.
    32768 = sum(binary:copy(<<16#A5>>, 8192), 0),
    io:format("ok~n").

check(256) -> ok;
check(N) ->
    B = <<N, (255-N), N>>,
    positions(B, N * 65536 + (255-N) * 256 + N, 0),
    check(N+1).

positions(_, _, 8) -> ok;
positions(B, N, Skip) ->
    Expected = N band ((1 bsl (24-Skip))-1),
    Expected = drop(B, Skip),
    Byte = (Expected bsr (16-Skip)) band 255,
    Byte = drop_byte(B, Skip),
    Byte = drop_case(B, Skip),
    Word = (Expected bsr (8-Skip)) band 65535,
    Word = drop_word(B, Skip),
    Little = ((Word band 255) bsl 8) bor (Word bsr 8),
    Little = drop_little(B, Skip),
    Signed = case Byte >= 128 of true -> Byte-256; false -> Byte end,
    Signed = drop_signed(B, Skip),
    positions(B, N, Skip+1).

drop(B, 0) -> collect(B, 0);
drop(<<_:1, R/bitstring>>, N) -> drop(R, N-1).
collect(<<>>, A) -> A;
collect(<<C:1, R/bitstring>>, A) -> collect(R, (A bsl 1) bor C).
sum(<<>>, A) -> A;
sum(<<C:1, R/bitstring>>, A) -> sum(R, A+C).

%% Terminal readers exercise the allocation-free BsStartMatch variant too.
drop_byte(B, 0) -> byte(B);
drop_byte(<<_:1, R/bitstring>>, N) -> drop_byte(R, N-1).
byte(<<C:8, _/bitstring>>) -> C.
drop_word(B, 0) -> word(B);
drop_word(<<_:1, R/bitstring>>, N) -> drop_word(R, N-1).
word(<<C:16, _/bitstring>>) -> C.
drop_little(B, 0) -> little(B);
drop_little(<<_:1, R/bitstring>>, N) -> drop_little(R, N-1).
little(<<C:16/little, _/bitstring>>) -> C.
drop_signed(B, 0) -> signed_byte(B);
drop_signed(<<_:1, R/bitstring>>, N) -> drop_signed(R, N-1).
signed_byte(<<C:8/signed, _/bitstring>>) -> C.
drop_case(B, 0) -> case_byte(B);
drop_case(<<_:1, R/bitstring>>, N) -> drop_case(R, N-1).
case_byte(B) -> case B of <<C:8, _/bitstring>> -> C end.
