%% EXPECT: binary to list domain passed
-module(binary_to_list_domain).
-export([test/0]).

test() ->
    [] = binary_to_list(<<>>),
    [0] = binary_to_list(<<0>>),
    [255] = binary_to_list(<<255>>),
    lists:foreach(fun(Length) ->
        Expected = lists:duplicate(Length, 165),
        Binary = list_to_binary(Expected),
        true = (binary_to_list(Binary) =:= Expected),
        %% A byte-aligned slice exercises a backing-owner offset for large data.
        WithPrefix = <<0, Binary/binary, 0>>,
        <<_:8, Slice:Length/binary, _/binary>> = WithPrefix,
        true = (binary_to_list(Slice) =:= Expected),
        lists:foreach(fun(Bits) ->
            expect_badarg(fun() -> binary_to_list(<<Binary/binary, 1:Bits>>) end),
            %% bitstring_to_list retains its distinct partial-bitstring behavior.
            true = (bitstring_to_list(<<Binary/binary, 1:Bits>>) =:= Expected ++ [<<1:Bits>>])
        end, lists:seq(1, 7))
    end, [0, 1, 16, 64, 65, 256]),
    lists:foreach(fun(Term) -> expect_badarg(fun() -> binary_to_list(Term) end) end,
        [[], 1, an_atom, {a}, #{}, fun() -> ok end]),
    io:format("binary to list domain passed~n"),
    ok.

expect_badarg(F) ->
    try F() of _ -> error(expected_badarg)
    catch error:badarg -> ok end.
