%% EXPECT: binary_tail_guard_fallback_ok
-module(binary_tail_guard_fallback).
-export([test/0]).

test() ->
    {1, <<46,48>>} = scan(<<51,46,48>>, 0),
    {1, <<32>>} = scan(<<51,32>>, 0),
    {1, <<44,32,51>>} = scan(<<51,44,32,51>>, 0),
    {1, <<13,10>>} = scan(<<51,13,10>>, 0),
    true = term_to_binary({1, <<46,48>>}) =:= term_to_binary(scan(<<51,46,48>>, 0)),
    {badmatch, {1, <<46,48>>}} = failure(<<51,46,48>>),
    true = term_to_binary({badmatch, {1, <<46,48>>}})
        =:= term_to_binary(failure(<<51,46,48>>)),
    true = term_to_binary(<<42>>) =:= term_to_binary(opaque(<<0,42>>)),
    true = term_to_binary(<<42>>) =:= builtin(<<0,42>>),
    true = term_to_binary(<<42>>) =:= remote(<<0,42>>),
    Bits = opaque_bits(<<0:1, 1:1, 0:1, 1:1>>),
    3 = bit_size(Bits),
    <<1:1, 0:1, 1:1>> = Bits,
    {0, <<46>>} = scan(<<46>>, 0),
    {2, <<>>} = scan(<<51,52>>, 0),
    io:format("binary_tail_guard_fallback_ok~n"),
    ok.

%% A failed byte guard must leave the whole input for the fallback clause.
scan(<<Byte, Rest/binary>>, Count) when Byte >= $0, Byte =< $9 ->
    scan(Rest, Count + 1);
scan(Rest, Count) ->
    {Count, Rest}.

failure(Bin) ->
    try
        {Count, <<>>} = scan(Bin, 0),
        Count
    catch error:Reason -> Reason end.

opaque(<<_, Rest/binary>>) -> identity(Rest).
identity(Value) -> Value.
builtin(<<_, Rest/binary>>) -> term_to_binary(Rest).
remote(<<_, Rest/binary>>) -> erlang:term_to_binary(Rest).

opaque_bits(<<_:1, Rest/bitstring>>) -> identity(Rest).
