%% EXPECT: ok
-module(math_sqrt).
-export([test/0]).
test() ->
    1.0 = result(1),
    Zero = result(0),
    <<0,0,0,0,0,0,0,0>> = <<Zero/float>>,
    2.0 = result(4),
    0.5 = result(0.25),
    Big = 1 bsl 200,
    Root = float(1 bsl 100),
    Root = result(Big),
    true = is_float(result(1 bsl 1023)),
    Tiny = result(1.0e-300),
    true = abs(Tiny / 1.0e-150 - 1.0) < 1.0e-14,
    Irrational = result(2),
    true = abs(Irrational*Irrational - 2.0) < 1.0e-14,
    NegativeZero = result(-0.0),
    <<128,0,0,0,0,0,0,0>> = <<NegativeZero/float>>,
    lists:foreach(fun(X) -> {exception,badarith} = result(X) end,
        [-1,-1.0,-Big,1 bsl 1024,-(1 bsl 1024)]),
    lists:foreach(fun(X) -> {exception,badarg} = result(X) end,
        [invalid,[],[1],{},<<>>,fun() -> ok end]),
    3.0 = math:sqrt(9),
    F = fun math:sqrt/1,
    4.0 = F(16),
    io:format("ok~n").
result(X) ->
    try apply(math,sqrt,[X]) of Value -> Value
    catch error:Reason -> {exception,Reason} end.
