%% EXPECT: rand_uniform_ok
%% Regression: rand:uniform/1 is missing, which stops Cowlib 2.22.0 from
%% building. It returns an integer in 1..N for any positive integer N,
%% including bignums, covers every value of a small range, and raises
%% function_clause for anything else, as on BEAM. The values are random, so the
%% checks are about range and coverage. Run this fixture on native Nera and BEAM.
-module(rand_uniform).
-export([test/0]).

test() ->
    1 = rand:uniform(id(1)),
    Draws = [rand:uniform(id(6)) || _ <- lists:seq(1, 600)],
    true = lists:all(fun(X) -> X >= 1 andalso X =< 6 end, Draws),
    [1, 2, 3, 4, 5, 6] = lists:usort(Draws),
    Large = id(1 bsl 62),
    true = lists:all(fun(X) -> is_integer(X) andalso X >= 1 andalso X =< Large end,
                     [rand:uniform(Large) || _ <- lists:seq(1, 100)]),
    Big = id(1 bsl 100),
    BigDraws = [rand:uniform(Big) || _ <- lists:seq(1, 200)],
    true = lists:all(fun(X) -> X >= 1 andalso X =< Big end, BigDraws),
    true = lists:any(fun(X) -> X > (1 bsl 64) end, BigDraws),
    true = length(lists:usort(BigDraws)) > 190,
    %% Dynamic calls.
    true = lists:member(apply(rand, uniform, [2]), [1, 2]),
    true = lists:member((fun rand:uniform/1)(id(2)), [1, 2]),
    Mod = id(rand), Fun = id(uniform),
    true = lists:member(Mod:Fun(id(2)), [1, 2]),
    lists:foreach(fun(Bad) ->
        function_clause = reason(fun() -> rand:uniform(id(Bad)) end),
        function_clause = reason(fun() -> apply(rand, uniform, [Bad]) end)
    end, [0, -1, -(1 bsl 100), 1.0, 0.5, foo, <<"1">>, [1], {1}]),
    io:format("rand_uniform_ok~n"),
    ok.

reason(F) ->
    try F() of
        Value -> {returned, Value}
    catch
        error:Reason -> Reason
    end.

id(X) -> X.
