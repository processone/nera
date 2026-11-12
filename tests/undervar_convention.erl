-module(undervar_convention).
-export([test/0]).

%% Tests for _Var convention and repeated variables in patterns.

test() ->
    %% 1. Plain _ is a wildcard (does not bind)
    _ = 42,
    _ = hello,

    %% 2. _Var binds the variable (unused but accessible)
    {_X, Y} = {10, 20},
    20 = Y,

    %% 3. _Var is accessible after binding
    _Z = 99,
    99 = _Z,

    %% 4. Repeated variable: must be equal
    {X1, X1} = {3, 3},
    3 = X1,

    %% 5. Repeated _Var: must be equal
    {_A, _A} = {7, 7},
    7 = _A,

    %% 6. Repeated variable in cons pattern
    [H, H] = [5, 5],
    5 = H,

    %% 7. Unequal repeated variable raises badmatch
    ok = try
        {V, V} = {1, 2},
        _ = V,
        fail
    catch
        error:{badmatch, _} -> ok
    end,

    %% 8. case_clause inside try-catch
    ok = try
        case id(99) of
            1 -> one;
            2 -> two
        end
    catch
        error:{case_clause, 99} -> ok
    end,

    %% 9. if_clause inside try-catch
    ok = try
        X2 = id(0),
        if
            X2 > 0 -> positive;
            X2 < 0 -> negative
        end
    catch
        error:if_clause -> ok
    end,

    io:format("ok~n"),
    ok.

id(I) -> I.
