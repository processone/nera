-module(binary_to_integer_base).
-export([test/0]).
test() ->
    255 = binary_to_integer(<<"ff">>, 16),
    -255 = binary_to_integer(<<"-FF">>, 16),
    35 = binary_to_integer(<<"+z">>, 36),
    0 = binary_to_integer(<<"-000">>, 2),
    18446744073709551615 = binary_to_integer(<<"ffffffffffffffff">>, 16),
    lists:foreach(fun(Base) ->
        lists:foreach(fun(Value) ->
            Bin = integer_to_binary(Value, Base),
            Value = binary_to_integer(Bin, Base),
            Value = apply(erlang, binary_to_integer, [Bin, Base])
        end, [0, 1, -1, 255, -255, 9223372036854775808, -9223372036854775809])
    end, lists:seq(2, 36)),
    lists:foreach(fun({Input, Base}) ->
        try binary_to_integer(Input, Base) of _ -> error(expected_badarg)
        catch error:badarg -> ok end
    end, [{<<>>, 10}, {<<"+">>, 10}, {<<"-">>, 10}, {<<" 1">>, 10},
          {<<"1 ">>, 10}, {<<"1_0">>, 10}, {<<"0xff">>, 16}, {<<"2">>, 2},
          {<<"z">>, 35}, {<<255>>, 16}, {<<49:7>>, 10}, {atom, 10},
          {[49], 10}, {<<"1">>, 1}, {<<"1">>, 37}, {<<"1">>, 2.0}, {<<"1">>, atom}]),
    ok.
