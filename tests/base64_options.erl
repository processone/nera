%% EXPECT: ok
-module(base64_options).
-export([test/0]).

test() ->
    Vectors = [{<<>>, <<>>}, {<<"f">>, <<"Zg==">>},
        {<<"fo">>, <<"Zm8=">>}, {<<"foo">>, <<"Zm9v">>},
        {<<"foobar">>, <<"Zm9vYmFy">>}],
    lists:foreach(fun({Raw, Enc}) ->
        Enc = base64:encode(Raw, #{}),
        Raw = base64:decode(Enc, #{}),
        Enc = base64:encode(binary_to_list(Raw), #{}),
        Raw = base64:decode(binary_to_list(Enc), #{})
    end, Vectors),
    <<"+//v">> = base64:encode(<<251,255,239>>, #{}),
    <<"-__v">> = base64:encode(<<251,255,239>>, #{mode => urlsafe}),
    <<251,255,239>> = base64:decode(<<"-__v">>, #{mode => urlsafe}),
    <<"Zg">> = base64:encode(<<"f">>, #{padding => false}),
    <<"f">> = base64:decode(<<"Zg">>, #{padding => false}),
    <<"f">> = base64:decode(<<"Zg=">>, #{padding => false}),
    <<"fo">> = base64:decode(<<"Zm8">>, #{padding => false}),
    <<"f">> = base64:decode(<<" Z\tg=\r=\n">>, #{}),
    <<"f">> = base64:decode(<<"Zh==">>, #{}),
    <<"fo">> = base64:decode(<<"Zm9=">>, #{}),
    All = list_to_binary(lists:seq(0,255)),
    lists:foreach(fun(Mode) -> lists:foreach(fun(Padding) ->
        Opts = #{mode => Mode, padding => Padding},
        All = base64:decode(base64:encode(All, Opts), Opts)
    end, [true,false]) end, [standard,urlsafe]),
    lists:foreach(fun(N) ->
        Raw = binary:part(All, 0, N),
        lists:foreach(fun(Pad) ->
            Opts = #{mode => urlsafe, padding => Pad},
            Raw = base64:decode(base64:encode(Raw, Opts), Opts)
        end, [true,false])
    end, lists:seq(0,256)),
    <<"Zg==">> = base64:encode(<<"f">>, #{unknown => ignored}),
    <<"Zg==">> = base64:encode(<<"f">>, #{mode => invalid, padding => invalid}),
    Large = binary:copy(All, 64),
    Large = base64:decode(base64:encode(Large, #{}), #{}),
    E = fun base64:encode/2, D = fun base64:decode/2,
    <<"Zg==">> = E("f", #{}), <<"f">> = D("Zg==", #{}),
    <<"Zg==">> = apply(base64,encode,[<<"f">>,#{}]),
    <<"f">> = apply(base64,decode,[<<"Zg==">>,#{}]),
    ok = error_is(missing_padding, fun() -> base64:decode(<<"Zg">>, #{}) end),
    ok = error_is(missing_padding, fun() -> base64:decode(<<"Zg=">>, #{}) end),
    ok = error_is(function_clause, fun() -> base64:decode(<<"A">>, #{}) end),
    ok = error_is(badarith, fun() -> base64:decode(<<"***=">>, #{}) end),
    ok = error_is(badarith, fun() -> base64:decode(<<"Y===">>, #{}) end),
    ok = error_is(badarg, fun() -> base64:decode(<<"*AAA">>, #{}) end),
    ok = error_is(badarg, fun() -> base64:decode(<<"-__v">>, #{}) end),
    ok = error_is(badarg, fun() -> base64:decode(<<"+//v">>, #{mode => urlsafe}) end),
    ok = error_is(badarg, fun() -> base64:decode(<<"=A">>, #{padding => false}) end),
    ok = error_is(badarg, fun() -> base64:decode(<<"A=A">>, #{padding => false}) end),
    ok = error_is(badarith, fun() -> base64:decode(<<"A*">>, #{padding => false}) end),
    ok = error_is({case_clause,eq}, fun() -> base64:decode(<<"Zg===">>, #{}) end),
    ok = error_is(function_clause, fun() -> base64:decode(42, #{}) end),
    ok = error_is(function_clause, fun() -> base64:encode(42, #{}) end),
    ok = error_is(function_clause, fun() -> base64:encode(<<>>, []) end),
    io:format("ok~n").

error_is(Expected, F) ->
    Actual = try F() of V -> {unexpected, V} catch error:R -> R end,
    Expected = Actual,
    ok.
