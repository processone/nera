%% EXPECT: ok
-module(case_clause_reason).
-export([test/0]).
reject(Value) ->
    case Value of accepted -> ok end.

test() ->
    {case_clause, 71} = reason(71),
    {case_clause, <<"bad">>} = reason(<<"bad">>),
    {case_clause, {nested, [1,2]}} = reason({nested, [1,2]}),
    {case_clause, #{key := [1|tail]}} = reason(#{key => [1|tail]}),
    {case_clause, <<5:3>>} = reason(<<5:3>>),
    {case_clause, []} = reason([]),
    {case_clause, 17} = failure(fun() -> guarded(17) end),
    ok = guarded(accepted),
    {case_clause, 23} = failure(fun() -> {prefix, reject(23)} end),
    undefined = put(case_evaluations, 0),
    {case_clause, {observed, 1}} = failure(fun() ->
        case observed() of accepted -> ok end
    end),
    1 = erase(case_evaluations),
    {case_clause, inner} = failure(fun() ->
        case rejected_outer of
            accepted -> ok;
            _ -> reject(inner)
        end
    end),
    ok = reject(accepted),
    io:format("ok~n").

reason(Value) ->
    try reject(Value) of _ -> unexpected_return
    catch error:Reason -> Reason end.

guarded(X) ->
    case X of V when V =:= accepted -> ok end.

observed() ->
    N = get(case_evaluations) + 1,
    put(case_evaluations, N),
    {observed, N}.

failure(F) ->
    try F() of _ -> unexpected_return
    catch error:R -> R end.
