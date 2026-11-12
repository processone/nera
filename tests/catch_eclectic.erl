%% Ported from OTP trycatch_SUITE concepts: eclectic catch — multiple catch
%% clauses with guards, patterns, and different exception classes.
-module(catch_eclectic).
-export([test/0]).

test() ->
    %% Guard in catch clause selects correct clause
    small_err = classify(error, 1),
    big_err   = classify(error, 100),
    small_thr = classify(throw, 2),
    big_thr   = classify(throw, 200),
    exited    = classify(exit, anything),
    %% Pattern in catch clause
    {tuple_err, a, b} = tuple_catch({a, b}),
    {list_err, [1,2,3]} = tuple_catch([1,2,3]),
    %% Rethrow if not matched — outer catches it
    rethrown = rethrow_unmatched(),
    %% Catch all with guard fallback
    fallback = with_fallback(9999),
    specific = with_fallback(42),
    %% Mixed classes with shared reason
    {error, r} = mixed_reasons(error, r),
    {throw, r} = mixed_reasons(throw, r),
    {exit, r}  = mixed_reasons(exit, r),
    ok.

classify(Class, Value) ->
    try
        case Class of
            error -> error(Value);
            throw -> throw(Value);
            exit  -> exit(Value)
        end
    catch
        error:V when is_integer(V), V < 10  -> small_err;
        error:V when is_integer(V), V >= 10 -> big_err;
        throw:V when is_integer(V), V < 10  -> small_thr;
        throw:V when is_integer(V), V >= 10 -> big_thr;
        exit:_                              -> exited
    end.

tuple_catch(V) ->
    try
        error(V)
    catch
        error:{A, B}     -> {tuple_err, A, B};
        error:L when is_list(L) -> {list_err, L}
    end.

rethrow_unmatched() ->
    try
        try
            error(nope)
        catch
            error:other -> should_not_match
        end
    catch
        error:nope -> rethrown
    end.

with_fallback(N) ->
    try
        error(N)
    catch
        error:42 -> specific;
        error:_  -> fallback
    end.

mixed_reasons(Class, Reason) ->
    try
        case Class of
            error -> error(Reason);
            throw -> throw(Reason);
            exit  -> exit(Reason)
        end
    catch
        C:R -> {C, R}
    end.
