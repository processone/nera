-module(bitstring_to_list).
-export([test/0]).
test() ->
    [] = bitstring_to_list(<<>>),
    [0, 255] = bitstring_to_list(<<0, 255>>),
    [255, 0, <<3:2>>] = bitstring_to_list(<<255, 0, 3:2>>),
    widths(1),
    Large = binary:copy(<<7>>, 128),
    Expected = lists:duplicate(128, 7) ++ [<<5:3>>],
    Expected = bitstring_to_list(<<Large/binary, 5:3>>),
    Expected = apply(erlang, bitstring_to_list, [<<Large/binary, 5:3>>]),
    lists:foreach(fun(X) ->
        try bitstring_to_list(X) of
            _ -> error(expected_badarg)
        catch error:badarg -> ok end
    end, [atom, [], [1, 2], 42, 1.5, {tuple}]),
    ok.
widths(8) -> ok;
widths(N) ->
    Bits = <<127:N>>,
    [Bits] = erlang:bitstring_to_list(Bits),
    BytePrefix = <<42, 127:N>>,
    BytePrefix = <<42, Bits/bitstring>>,
    BitPrefix = <<5:3, 127:N>>,
    BitPrefix = <<5:3, Bits/bitstring>>,
    [42, Bits] = bitstring_to_list(<<42, Bits/bitstring>>),
    widths(N + 1).
