%% EXPECT: ok
%% The same test runs unchanged against OTP's base64 module.
-module(base64_large_encode).
-export([test/0]).
test() ->
    lists:foreach(fun(N) ->
        Raw = binary:copy(<<251,255,239>>, N),
        lists:foreach(fun({Mode, Symbol}) ->
            Prefix = binary:copy(Symbol, N),
            lists:foreach(fun({Tail, Padded, Bare}) ->
                Wrapped = <<42,Raw/binary,Tail/binary,43>>,
                Input = binary:part(Wrapped, 1, byte_size(Wrapped)-2),
                lists:foreach(fun({Pad, Suffix}) ->
                    Expected = <<Prefix/binary,Suffix/binary>>,
                    Options = #{mode => Mode, padding => Pad},
                    Expected = base64:encode(Input, Options),
                    Expected = apply(base64, encode, [Input, Options]),
                    F = fun base64:encode/2,
                    Expected = F(Input, Options),
                    %% Preserve live caller roots and exception handling over suspension.
                    Expected = try base64:encode(Input, Options) of
                        V -> V
                    catch C:R -> error({unexpected,C,R}) end,
                    42 = binary:at(Wrapped, 0),
                    Expected = receive_result(Input, Options)
                end, [{true,Padded},{false,Bare}])
            end, [{<<>>,<<>>,<<>>},{<<"f">>,<<"Zg==">>,<<"Zg">>},
                  {<<"fo">>,<<"Zm8=">>,<<"Zm8">>}])
        end, [{standard,<<"+//v">>},{urlsafe,<<"-__v">>}])
    end, [0,1,16383,16384,16385,700001]),
    io:format("ok~n").
receive_result(Input, Options) ->
    Parent = self(),
    spawn(fun() -> Parent ! {encoded,base64:encode(Input,Options)} end),
    receive {encoded,Result} -> Result after 10000 -> error(encode_timeout) end.
