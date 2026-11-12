%% EXPECT: unicode list scalar domain passed
-module(unicode_list_scalar_domain).
-export([test/0]).

test() ->
    Scalars = [0,65,127,128,16#7ff,16#800,16#d7ff,16#e000,16#ffff,16#10000,16#10ffff],
    true = (atom_to_list(list_to_atom(Scalars)) =:= Scalars),
    lists:foreach(fun(Value) ->
        expect_badarg(fun() -> list_to_atom([Value]) end)
    end,[-1,16#d800,16#dfff,16#110000,16#100000041,16#100000031,
          16#10010ffff,16#10000000041,1152921504606846975,1 bsl 100]),
    lists:foreach(fun(Value) ->
        expect_badarg(fun() -> list_to_integer([Value]) end)
    end,[-1,16#d800,16#110000,16#100000041,1 bsl 100]),
    expect_badarg(fun() -> list_to_atom([not_an_integer]) end),
    expect_badarg(fun() -> list_to_atom([65|not_a_list]) end),
    true = (list_to_integer("12345") =:= 12345),
    io:format("unicode list scalar domain passed~n"),
    ok.

expect_badarg(F) ->
    try F() of _ -> error(expected_badarg)
    catch error:badarg -> ok end.
