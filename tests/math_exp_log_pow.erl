%% EXPECT: math_exp_log_pow_ok
%% Regression: math:exp/1, math:log/1 and math:pow/2 were missing, and rand's
%% normal and uniform_real draws call them. Results and error reasons are
%% OTP 29's. Values that the C library rounds are compared to a relative
%% 1.0e-15; exact ones must be identical. Run this fixture on native Nera and
%% BEAM.
-module(math_exp_log_pow).
-export([test/0]).

test() ->
    Big = 1 bsl 100,
    Huge = 1 bsl 2000,
    %% exp/1
    1.0 = call(exp, [0]),
    positive_zero(call(exp, [-1000])),
    near(2.718281828459045, call(exp, [1])),
    near(8.218407461554972e307, call(exp, [709])),
    near(0.36787944117144233, call(exp, [-1.0])),
    %% log/1
    positive_zero(call(log, [1])),
    near(69.31471805599453, call(log, [Big])),
    near(-744.4400719213812, call(log, [5.0e-324])),
    near(2.302585092994046, call(log, [10])),
    %% pow/2
    1024.0 = call(pow, [2, 10]),
    5.0e-324 = call(pow, [2.0, -1074]),
    1.0 = call(pow, [0, 0]),
    -8.0 = call(pow, [-2, 3]),
    0.25 = call(pow, [2, -2]),
    near(1.4142135623730951, call(pow, [2, 0.5])),
    near(1.2676506002282294e30, call(pow, [Big, 1])),
    %% Overflow, domain errors and numbers outside the float range.
    lists:foreach(fun(Args) -> {error, badarith} = call(exp, Args) end,
                  [[710], [Huge], [-Huge], [Big]]),
    lists:foreach(fun(Args) -> {error, badarith} = call(log, Args) end,
                  [[0], [0.0], [-1], [-1.0e-300], [Huge]]),
    lists:foreach(fun(Args) -> {error, badarith} = call(pow, Args) end,
                  [[2, 1024], [-8, 1 / 3], [0, -1], [-0.0, -1], [Huge, 0], [2, Huge]]),
    %% Nonnumbers raise badarg, before any range check.
    lists:foreach(fun(Bad) ->
                          {error, badarg} = call(exp, [Bad]),
                          {error, badarg} = call(log, [Bad]),
                          {error, badarg} = call(pow, [Bad, 2]),
                          {error, badarg} = call(pow, [2, Bad]),
                          {error, badarg} = call(pow, [Bad, Huge])
                  end, [foo, [], [1], {}, <<>>, fun() -> ok end]),
    %% Direct, remote fun and variable module calls.
    1.0 = math:exp(0),
    positive_zero(math:log(1)),
    8.0 = math:pow(2, 3),
    Pow = fun math:pow/2,
    9.0 = Pow(3, 2),
    Mod = id(math),
    positive_zero(Mod:log(id(1))),
    io:format("math_exp_log_pow_ok~n"),
    ok.

call(F, Args) ->
    try apply(math, F, Args) of
        Value -> Value
    catch
        error:Reason -> {error, Reason}
    end.

positive_zero(X) ->
    <<0:64>> = <<X/float>>,
    ok.

near(Expected, Actual) when is_float(Actual) ->
    true = abs(Actual - Expected) =< 1.0e-15 * abs(Expected),
    ok.

id(X) -> X.
