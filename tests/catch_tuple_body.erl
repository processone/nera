-module(catch_tuple_body).
-export([test/0]).

test() ->
    %% throw(Expr) still works as before
    ok = test_throw_call(),
    %% {throw, Reason} in catch clause body
    ok = test_catch_tuple(),
    %% bare throw as atom
    throw = id(throw),
    %% throw in a list
    [throw, error, exit] = [throw, error, exit],
    %% throw as first element of tuple outside catch
    {throw, hello} = {throw, hello},
    %% `throw` as a bare atom in pattern position (case clause)
    thrown = classify(throw),
    erred  = classify(error),
    exited = classify(exit),
    other  = classify(other),
    %% `throw` as a bare atom in pattern position (function head)
    from_throw = atom_kind(throw),
    from_exit  = atom_kind(exit),
    ok.

classify(X) ->
    case X of
        throw -> thrown;
        error -> erred;
        exit  -> exited;
        _     -> other
    end.

atom_kind(throw) -> from_throw;
atom_kind(exit)  -> from_exit;
atom_kind(_)     -> from_other.

test_throw_call() ->
    Result = try throw(boom)
    catch
        throw:R -> {caught, R}
    end,
    {caught, boom} = Result,
    ok.

test_catch_tuple() ->
    Result = try throw(oops)
    catch
        throw:Reason -> {throw, Reason}
    end,
    {throw, oops} = Result,
    ok.

id(X) -> X.
