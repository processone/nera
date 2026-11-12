%% EXPECT: binary_longest_common_prefix_ok
%% cow_hpack's original indexing tests execute binary:longest_common_prefix/1.
-module(binary_longest_common_prefix).
-export([test/0]).

test() ->
    2 = binary:longest_common_prefix([<<"abx">>, <<"aby">>]),
    0 = binary:longest_common_prefix([<<>>, <<"abc">>]),
    0 = binary:longest_common_prefix([<<"abc">>, <<"xyz">>]),
    3 = binary:longest_common_prefix([<<"abc">>]),
    3 = binary:longest_common_prefix([<<"abc">>, <<"abcd">>, <<"abc">>]),
    F = fun binary:longest_common_prefix/1,
    2 = F([<<0,255,3>>, <<0,255,4>>]),
    1 = apply(binary, longest_common_prefix, [[<<"a1">>, <<"a2">>]]),
    Large = binary:copy(<<42>>, 10000),
    10000 = binary:longest_common_prefix([Large, <<Large/binary,1>>]),
    <<_:3, Shifted:80/binary, _/bits>> = <<0:3, Large/binary, 0:5>>,
    80 = binary:longest_common_prefix([Shifted, Large]),
    lists:foreach(fun(Input) ->
        badarg = reason(fun() -> F(Input) end)
    end, [[], not_a_list, <<>>, [no_binary], [<<>>,no_binary],
          [<<"a">>,<<"b">>,no_binary], [<<1:1>>], [<<"abc">>|bad_tail]]),
    io:format("binary_longest_common_prefix_ok~n"),
    ok.
reason(F) -> try F() of Value -> {unexpected, Value} catch error:Reason -> Reason end.
