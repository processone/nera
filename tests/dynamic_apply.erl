-module(dynamic_apply).
-export([test/0]).

%% Test dynamic remote calls: Mod:Fun(Args) syntax

test() ->
    test_var_module(),
    test_var_function(),
    test_var_both(),
    test_bif_dispatch(),
    test_cross_module_stdlib(),
    test_try_catch(),
    test_tail_position(),
    test_undef_error(),
    ok.

%% Var:atom(args) — variable module, atom function
test_var_module() ->
    M = dynamic_apply,
    ok = M:helper_add(1, 2, 3).

%% atom:Var(args) — atom module, variable function
test_var_function() ->
    F = helper_mul,
    ok = dynamic_apply:F(2, 3, 6).

%% Var:Var(args) — both variable
test_var_both() ->
    M = dynamic_apply,
    F = helper_id,
    42 = M:F(42).

%% BIFs via apply dispatch
test_bif_dispatch() ->
    M = erlang,
    3 = M:length([a, b, c]),
    true = M:is_atom(hello),
    [b, a] = M:tl([c, b, a]).

%% Cross-module stdlib function
test_cross_module_stdlib() ->
    M = lists,
    [3, 2, 1] = M:reverse([1, 2, 3]).

%% Dynamic call inside try/catch — exception propagation
test_try_catch() ->
    M = dynamic_apply,
    F = helper_crash,
    Result = try M:F(boom) of
        _ -> no_error
    catch
        error:boom -> caught_error
    end,
    caught_error = Result.

%% Dynamic call in tail position
test_tail_position() ->
    M = dynamic_apply,
    99 = tail_caller(M, 99).

%% Dynamic call to nonexistent function raises error:undef
test_undef_error() ->
    M = dynamic_apply,
    F = nonexistent_function,
    Result = try M:F(1) of
        _ -> no_error
    catch
        error:undef -> caught_undef
    end,
    caught_undef = Result.

%% --- helpers ---

helper_add(A, B, Expected) ->
    Expected = A + B,
    ok.

helper_mul(A, B, Expected) ->
    Expected = A * B,
    ok.

helper_id(X) -> X.

helper_crash(Reason) -> error(Reason).

tail_caller(Mod, X) ->
    Mod:helper_id(X).
