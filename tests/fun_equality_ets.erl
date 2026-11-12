%% Tests: fun comparison, funs as map values, funs in ETS.
-module(fun_equality_ets).
-export([test/0]).

test() ->
    %% Same fun reference is equal to itself
    F1 = fun(X) -> X + 1 end,
    true = (F1 == F1),

    %% Different closures with same code but different env
    F2 = make_adder(1),
    F3 = make_adder(2),
    3 = F2(2),
    4 = F3(2),

    %% Fun in tuple
    T = {inc, F2},
    {inc, Fn} = T,
    5 = Fn(4),

    %% Fun in list
    Funs = [fun(X) -> X * 2 end, fun(X) -> X * 3 end],
    [4, 9] = apply_each(Funs, [2, 3]),

    %% Fun as map value
    M = #{add => fun(A, B) -> A + B end, mul => fun(A, B) -> A * B end},
    AddFn = maps:get(add, M),
    MulFn = maps:get(mul, M),
    5 = AddFn(2, 3),
    6 = MulFn(2, 3),

    %% Fun in ETS
    Tab = ets:new(funs, [set]),
    ets:insert(Tab, {inc, F2}),
    ets:insert(Tab, {double, fun(X) -> X * 2 end}),
    [{inc, IncFn}] = ets:lookup(Tab, inc),
    [{double, DblFn}] = ets:lookup(Tab, double),
    11 = IncFn(10),
    20 = DblFn(10),

    %% Overwrite fun in ETS
    F4 = make_adder(100),
    ets:insert(Tab, {inc, F4}),
    [{inc, IncFn2}] = ets:lookup(Tab, inc),
    110 = IncFn2(10),

    ets:delete(Tab),

    %% Fun arity check (is_function/1 only — /2 not yet supported)
    F5 = fun(X, Y) -> X + Y end,
    true = is_function(F5),

    io:format("ok~n"),
    ok.

make_adder(N) ->
    fun(X) -> X + N end.

apply_each([], []) -> [];
apply_each([F|Fs], [A|As]) ->
    [F(A) | apply_each(Fs, As)].
