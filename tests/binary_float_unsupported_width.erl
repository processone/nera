%% OTP 29 compiles unsupported float widths: construction raises at runtime,
%% and patterns never match, even when the extracted value is ignored.
%% EXPECT: binary_float_unsupported_width_ok
-module(binary_float_unsupported_width).
-export([test/0]).

test() ->
    lists:foreach(fun(F) -> badarg = outcome(F) end, [
        fun() -> <<1.0:0/float>> end,
        fun() -> <<1.0:3/float>> end,
        fun() -> <<1.0:8/float>> end,
        fun() -> <<1.0:24/float-little>> end,
        fun() -> <<1.0:48/float-native>> end,
        fun() -> <<1.0:128/float>> end,
        fun() -> <<1.0:3/float-unit:8>> end,
        fun() -> <<1.0:2/float-unit:64>> end,
        fun() -> <<"A":24/float>> end
    ]),
    %% An unsupported size must not prevent evaluation of later segments.
    erase(trace),
    badarg = outcome(fun() -> <<(mark(value, 1.0)):24/float,
                              (mark(later, 7)):8>> end),
    [later, value] = erase(trace),
    badarith = outcome(fun() -> <<1.0:24/float, (1 div id(0)):8>> end),
    erase(continued),
    caught = try
        <<1.0:24/float>>,
        put(continued, true)
    catch error:badarg -> caught
    end,
    undefined = erase(continued),
    fallback = if <<1.0:24/float>> =:= <<0:24>> -> matched;
                  true -> fallback end,
    lists:foreach(fun(Bin) ->
        fallback = case Bin of <<_:3/float>> -> matched; _ -> fallback end,
        fallback = case Bin of <<_:8/float>> -> matched; _ -> fallback end,
        fallback = case Bin of <<_:24/float-little>> -> matched; _ -> fallback end,
        fallback = case Bin of <<_:48/float-native>> -> matched; _ -> fallback end,
        fallback = case Bin of <<_:128/float>> -> matched; _ -> fallback end,
        fallback = head(Bin),
        fallback = scaled_head(Bin),
        F = fun(<<X:24/float>>) -> X; (_) -> fallback end,
        fallback = F(Bin),
        {badmatch, Bin} = outcome(fun() -> <<_:24/float>> = Bin end)
    end, [<<>>, <<0:3>>, <<0:8>>, <<0:24>>, <<0:48>>, <<0:128>>]),
    [] = [X || <<X:24/float>> <- [<<0:24>>, <<0:48>>]],
    [] = [X || <<X:24/float>> <= <<0:48>>],
    [] = [matched || <<_:3/float-unit:8>> <= <<0:48>>],
    <<>> = << <<X:64/float>> || <<X:24/float>> <= <<0:48>> >>,
    self() ! <<0:24>>,
    fallback = receive <<_:24/float>> -> matched after 0 -> fallback end,
    receive <<0:24>> -> ok after 0 -> error(message_consumed) end,
    %% Supported sizes, including size times unit, keep their encoded values.
    <<16#3C, 0>> = <<1.0:1/float-unit:16>>,
    <<16#3F, 16#80, 0, 0>> = <<1.0:4/float-unit:8>>,
    <<16#3F, 16#F0, 0, 0, 0, 0, 0, 0>> = <<1.0:2/float-unit:32>>,
    {1.0, 1.0, 1.0} = valid(<<1.0:16/float, 1.0:32/float, 1.0:64/float>>),
    io:format("binary_float_unsupported_width_ok~n"),
    ok.

head(<<X:24/float>>) -> X;
head(_) -> fallback.
scaled_head(<<X:3/float-unit:8>>) -> X;
scaled_head(<<X:2/float-unit:64>>) -> X;
scaled_head(_) -> fallback.
valid(<<H:1/float-unit:16, S:4/float-unit:8, D:2/float-unit:32>>) -> {H, S, D}.

outcome(F) ->
    try F() of
        Value -> {unexpected_return, Value}
    catch error:Reason -> Reason
    end.
mark(Label, Value) ->
    Trace = case get(trace) of undefined -> []; Previous -> Previous end,
    put(trace, [Label | Trace]),
    Value.
id(X) -> X.
