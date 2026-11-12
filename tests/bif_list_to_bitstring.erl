%% EXPECT: bif_list_to_bitstring_ok
%% Regression: list_to_bitstring/1 joins bytes and bitstrings, nested in
%% lists, bit by bit, as on BEAM. Before the fix, calls naming it directly
%% failed to compile as an unresolved function, and calls through apply or
%% variable names raised undef, although binary comprehensions already used
%% the same runtime join. Direct, erlang:-qualified, apply and fun
%% reference calls must all reach it and raise badarg on the same inputs.
%% Run this same fixture on native Nera and BEAM.
-module(bif_list_to_bitstring).
-export([test/0]).

test() ->
    %% Direct calls: partial bytes pack without padding.
    <<34>> = list_to_bitstring(id([<<1:3>>, <<2:5>>])),
    <<1, 2:4>> = list_to_bitstring(id([1, <<2:4>>])),
    <<"ab", 1:1>> = list_to_bitstring(id(["ab", <<1:1>>])),
    <<>> = list_to_bitstring(id([])),
    <<>> = list_to_bitstring(id([[], [[]], <<>>])),

    %% A result that ends inside a byte is a bitstring, not a binary.
    Bits = list_to_bitstring(id([<<1:1>>, <<0:1>>, <<1:1>>])),
    <<5:3>> = Bits,
    3 = bit_size(Bits),
    false = is_binary(Bits),
    true = is_binary(list_to_bitstring(id([1, 2, 3]))),
    <<1, 2, 3>> = list_to_bitstring(id([1, 2, 3])),

    %% Nested lists and bitstring tails, at any depth.
    <<1, 5:3>> = list_to_bitstring(id([1 | <<5:3>>])),
    <<7:3>> = list_to_bitstring(id([[<<1:1>> | <<1:1>>] | <<1:1>>])),
    <<1, 2, 3, 4, 5:4>> = list_to_bitstring(id([[1, [2, [3]]], [[[4]]], <<5:4>>])),
    <<16#AB, 16#CD, 16#EF>> = list_to_bitstring(id([<<16#ABC:12>>, <<16#DEF:12>>])),

    %% erlang:-qualified calls.
    <<34>> = erlang:list_to_bitstring(id([<<1:3>>, <<2:5>>])),
    <<1, 5:3>> = erlang:list_to_bitstring(id([1 | <<5:3>>])),

    %% apply/2,3 and variable module and function names.
    Input = [<<1:3>>, [<<2:5>>, 255], <<1:1>>],
    Expected = <<34, 255, 1:1>>,
    Expected = apply(erlang, list_to_bitstring, [id(Input)]),
    Expected = erlang:apply(id(erlang), id(list_to_bitstring), [id(Input)]),
    Expected = apply(fun list_to_bitstring/1, [id(Input)]),
    Mod = id(erlang),
    Fun = id(list_to_bitstring),
    Expected = Mod:Fun(id(Input)),

    %% Fun references: auto-imported, qualified and with variable names.
    Local = fun list_to_bitstring/1,
    Remote = fun erlang:list_to_bitstring/1,
    Dynamic = fun Mod:Fun/1,
    Expected = Local(id(Input)),
    Expected = Remote(id(Input)),
    Expected = Dynamic(id(Input)),
    [<<1:1>>, <<1, 2:4>>] = lists:map(Local, id([[<<1:1>>], [1, <<2:4>>]])),

    %% Results above the inline binary size keep every bit.
    Long = list_to_bitstring(id(lists:duplicate(70, <<255, 1:1>>))),
    630 = bit_size(Long),
    true = Long =:= repeat(70, <<255, 1:1>>, <<>>),
    Ones = erlang:list_to_bitstring(id(lists:duplicate(1001, <<1:1>>))),
    true = Ones =:= <<((1 bsl 1001) - 1):1001>>,

    %% Pieces taken from inside a larger binary.
    Big = id(list_to_binary(lists:duplicate(100, 16#A5))),
    <<_:3, Rest/bits>> = Big,
    Twice = list_to_bitstring(id([Rest, Rest])),
    true = Twice =:= <<Rest/bits, Rest/bits>>,
    Shared = tail(Big),
    Joined = Remote(id([Shared, <<1:1>> | Shared])),
    true = Joined =:= <<Shared/binary, 1:1, Shared/binary>>,

    %% Inputs BEAM rejects raise badarg through every call form.
    badargs([atom, 42, <<1>>, <<1:1>>, {[1]}, 1.5,
             [256], [-1], [a], [1.5], [{1}], [1 | a], [<<1>> | 2],
             [[1 | b]], [<<1:1>>, "ab" | foo]]),

    io:format("bif_list_to_bitstring_ok~n"),
    ok.

badargs([]) -> ok;
badargs([Bad | More]) ->
    Mod = id(erlang),
    Fun = id(list_to_bitstring),
    badarg = reason(fun() -> list_to_bitstring(id(Bad)) end),
    badarg = reason(fun() -> erlang:list_to_bitstring(id(Bad)) end),
    badarg = reason(fun() -> apply(Mod, Fun, [id(Bad)]) end),
    badarg = reason(fun() -> apply(fun list_to_bitstring/1, [id(Bad)]) end),
    badarg = reason(fun() -> Mod:Fun(id(Bad)) end),
    badarg = reason(fun() -> (fun list_to_bitstring/1)(id(Bad)) end),
    badarg = reason(fun() -> (fun erlang:list_to_bitstring/1)(id(Bad)) end),
    badarg = reason(fun() -> (fun Mod:Fun/1)(id(Bad)) end),
    badargs(More).

reason(F) ->
    try F() of
        Value -> {returned, Value}
    catch
        error:Reason -> Reason
    end.

repeat(0, _Piece, Acc) -> Acc;
repeat(N, Piece, Acc) -> repeat(N - 1, Piece, <<Acc/bits, Piece/bits>>).

tail(<<_:8, Tail/binary>>) -> Tail.

id(X) -> X.
