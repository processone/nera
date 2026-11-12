%% EXPECT: bif_list_to_integer_base_ok
%% Regression: erlang:list_to_integer/2 is missing, which stops Cowlib 2.22.0
%% from building. It parses a flat character list in bases 2 to 36 like
%% binary_to_integer/2 and raises badarg on the same inputs as BEAM through
%% direct, qualified, apply and fun reference calls. Run this fixture on
%% native Nera and BEAM.
-module(bif_list_to_integer_base).
-export([test/0]).

test() ->
    255 = list_to_integer(id("ff"), 16),
    255 = list_to_integer(id("FF"), 16),
    -255 = list_to_integer(id("-ff"), 16),
    255 = erlang:list_to_integer(id("+Ff"), 16),
    35 = list_to_integer(id("z"), 36),
    0 = list_to_integer(id("-000"), 2),
    10 = list_to_integer(id("10"), 10),
    18446744073709551615 = list_to_integer(id("ffffffffffffffff"), 16),
    -170141183460469231731687303715884105728 =
        list_to_integer(id("-80000000000000000000000000000000"), 16),
    lists:foreach(fun(Base) ->
        lists:foreach(fun(Value) ->
            Text = integer_to_list(Value, Base),
            Value = list_to_integer(Text, Base),
            Value = apply(erlang, list_to_integer, [Text, Base]),
            Value = (fun list_to_integer/2)(Text, Base),
            Value = (fun erlang:list_to_integer/2)(Text, Base),
            Mod = id(erlang), Fun = id(list_to_integer),
            Value = Mod:Fun(Text, Base)
        end, [0, 1, -1, 255, -255, 9223372036854775808, -9223372036854775809])
    end, lists:seq(2, 36)),
    lists:foreach(fun({Input, Base}) ->
        badarg = reason(fun() -> list_to_integer(id(Input), id(Base)) end),
        badarg = reason(fun() -> apply(erlang, list_to_integer, [Input, Base]) end)
    end, [{[], 10}, {"+", 10}, {"-", 10}, {" 1", 10}, {"1 ", 10},
          {"1_0", 10}, {"0xff", 16}, {"2", 2}, {"z", 35}, {"1.0", 10},
          {[256], 10}, {[-1], 10}, {[$1 | $2], 10}, {[[$1]], 10},
          {"1", 1}, {"1", 37}, {"1", 0}, {"1", 2.0}, {"1", atom},
          {<<"1">>, 10}, {atom, 10}, {12, 10}]),
    io:format("bif_list_to_integer_base_ok~n"),
    ok.

reason(F) ->
    try F() of
        Value -> {returned, Value}
    catch
        error:Reason -> Reason
    end.

id(X) -> X.
