%% Ported from OTP trycatch_SUITE concepts: catch-in-catch, try-in-try.
-module(catch_in_catch).
-export([test/0]).

test() ->
    %% catch-expression nested inside catch-clause body
    {outer, {'EXIT', inner_exit}} = outer_sees_inner(),
    %% exception raised inside a catch-clause of an outer try is caught
    %% by outer's own catch when there's another layer.
    wrapped = inner_raises_outer_catches(),
    %% catch inside try..catch clause still works
    {caught_inner, oops} = catch_inside_clause(),
    %% Exception thrown in an `of` branch is NOT caught by the SAME try's
    %% catch clauses (per Erlang semantics — only exceptions from Body
    %% go to that try's catch). It propagates to the outer try.
    escaped_to_outer = thrown_in_of_branch(),
    %% Same: exception from inner's `of` branch bypasses inner's catch
    %% and is caught by the outer try.
    caught_outer = of_exception_bypasses_inner(),
    ok.

outer_sees_inner() ->
    Result = try
        error(top)
    catch
        error:top ->
            Inner = (catch exit(inner_exit)),
            {outer, Inner}
    end,
    Result.

inner_raises_outer_catches() ->
    try
        try
            throw(first)
        catch
            throw:first -> error(second)
        end
    catch
        error:second -> wrapped
    end.

catch_inside_clause() ->
    try
        error(top)
    catch
        error:top ->
            Result = try
                throw(oops)
            catch
                throw:R -> {caught_inner, R}
            end,
            Result
    end.

thrown_in_of_branch() ->
    try
        try
            ok
        of
            ok -> throw(of_boom)
        catch
            throw:of_boom -> wrong_same_try
        end
    catch
        throw:of_boom -> escaped_to_outer
    end.

of_exception_bypasses_inner() ->
    try
        try
            ok
        of
            ok -> throw(bypass)
        catch
            throw:something_else -> wrong
        end
    catch
        throw:bypass -> caught_outer
    end.
