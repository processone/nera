%% EXPECT: ok
%% Nera's internal BIF must exist: this cannot pass via the Erlang fallback.
-module(base64_native_decode).
-export([test/0]).
test() ->
    {ok,<<"f">>} = try apply(base64, decode_binary, [<<"Zg==">>,false,true]) of
        Value -> {ok,Value}
    catch C:R -> {C,R} end,
    {ok,<<"f">>} = try apply(base64, decode_list, ["Zg==",false,true]) of
        ListValue -> {ok,ListValue}
    catch LC:LR -> {LC,LR} end,
    io:format("ok~n").
