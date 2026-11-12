%% EXPECT: ETF container bounds passed
-module(etf_container_bounds).
-export([test/0]).

test() ->
    lists:foreach(fun(Binary) ->
        expect_badarg(fun() -> binary_to_term(Binary) end)
    end, [
        <<131, 104, 255>>,
        <<131, 105, 16#ffffffff:32>>,
        <<131, 108, 16#ffffffff:32, 106>>,
        <<131, 116, 16#ffffffff:32>>,
        <<131, 108, 0:32>>,                 %% Empty list still requires a tail.
        <<131, 108, 1:32, 106>>,            %% Element without tail.
        <<131, 116, 1:32, 106>>,            %% Key without value.
        <<131, 105, 2:32, 106>>,            %% Missing tuple element.
        <<131, 104, 1, 108, 16#ffffffff:32, 106>> %% Nested impossible count.
    ]),
    %% NIL_EXT needs just one byte, including as a map key or value.
    {} = binary_to_term(<<131, 104, 0>>),
    {} = binary_to_term(<<131, 105, 0:32>>),
    [] = binary_to_term(<<131, 108, 0:32, 106>>),
    #{} = binary_to_term(<<131, 116, 0:32>>),
    {[]} = binary_to_term(<<131, 104, 1, 106>>),
    {[]} = binary_to_term(<<131, 105, 1:32, 106>>),
    [[]] = binary_to_term(<<131, 108, 1:32, 106, 106>>),
    [[]|7] = binary_to_term(<<131, 108, 1:32, 106, 97, 7>>),
    #{[] := []} = binary_to_term(<<131, 116, 1:32, 106, 106>>),
    lists:foreach(fun(Term) ->
        true = (binary_to_term(term_to_binary(Term)) =:= Term)
    end, [lists:duplicate(255, []), list_to_tuple(lists:duplicate(256, [])),
          {[], {}, #{a => {1, 2}}, [1, atom, <<3>>]}]),
    io:format("ETF container bounds passed~n"),
    ok.

expect_badarg(F) ->
    try F() of _ -> error(expected_badarg)
    catch error:badarg -> ok end.
