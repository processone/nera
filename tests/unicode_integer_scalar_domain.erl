%% EXPECT: strict integer scalar domain passed
%% Deliberate difference from OTP 29: invalid >u32 values must not alias digits.
-module(unicode_integer_scalar_domain).
-export([test/0]).

test() ->
    lists:foreach(fun(Value) ->
        expect_badarg(fun() -> list_to_integer([Value]) end)
    end,[16#100000030,16#100000031,16#100000039,16#10000002b,16#10000002d]),
    true = (list_to_integer("123") =:= 123),
    true = (list_to_integer("-123") =:= -123),
    io:format("strict integer scalar domain passed~n"),
    ok.
expect_badarg(F) ->
    try F() of _ -> error(expected_badarg)
    catch error:badarg -> ok end.
