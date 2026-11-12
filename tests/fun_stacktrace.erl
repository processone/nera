%% Tests that stacktrace entries for fun calls have the correct shape.
%% In OTP, a fun call frame is {Module, Function, Arity, Info} where
%% Function may be a compiler-generated name like '-test/0-fun-0-'.
%% We verify the structural properties without assuming exact names.
-module(fun_stacktrace).
-export([test/0]).

test() ->
    ok = simple_fun_error(),
    ok = closure_error(),
    ok = nested_fun_error(),
    ok = multi_clause_fun_error(),
    ok = fun_called_via_apply(),
    ok = named_fun_error(),
    ok = fun_ref_error(),
    ok = catch_expr_fun_error(),
    ok.

%% --- 1. Simple anonymous fun raising an error ---
%% The stacktrace should contain a frame for the fun itself.
simple_fun_error() ->
    F = fun() -> error(boom) end,
    {error, boom, Stk} = get_error(F, []),
    true = is_list(Stk),
    true = length(Stk) > 0,
    %% Top frame should be a 4-tuple {Mod, Fun, Arity, Info}
    {Mod, _FunName, Arity, Info} = hd(Stk),
    true = is_atom(Mod),
    true = is_integer(Arity),
    true = is_list(Info),
    ok.

%% --- 2. Closure raising an error preserves stacktrace shape ---
closure_error() ->
    V = id(42),
    F = fun() -> error({captured, V}) end,
    {error, {captured, 42}, Stk} = get_error(F, []),
    true = length(Stk) > 0,
    {Mod, _, Arity, Info} = hd(Stk),
    true = is_atom(Mod),
    true = is_integer(Arity),
    true = is_list(Info),
    ok.

%% --- 3. Nested fun: inner fun raises, stacktrace has multiple frames ---
nested_fun_error() ->
    Outer = fun() ->
        Inner = fun() -> error(inner_boom) end,
        Inner()
    end,
    {error, inner_boom, Stk} = get_error(Outer, []),
    true = length(Stk) >= 2,
    %% Each frame is a 4-tuple
    lists:foreach(fun(Frame) ->
        {M, F, A, I} = Frame,
        true = is_atom(M),
        true = is_integer(A),
        true = is_list(I),
        %% F is usually an atom (compiler-generated name)
        true = is_atom(F)
    end, Stk),
    ok.

%% --- 4. Multi-clause fun: function_clause error includes the fun frame ---
multi_clause_fun_error() ->
    F = fun(1) -> one;
           (2) -> two
        end,
    {error, function_clause, Stk} = get_error(F, [99]),
    true = length(Stk) > 0,
    {_, _, _, _} = hd(Stk),
    ok.

%% --- 5. Fun called via apply/2 still gets proper stacktrace ---
fun_called_via_apply() ->
    F = fun(_X) -> error(applied_boom) end,
    Stk = try
        apply(F, [hello])
    catch
        error:applied_boom:S -> S
    end,
    true = is_list(Stk),
    true = length(Stk) > 0,
    {_, _, _, _} = hd(Stk),
    ok.

%% --- 6. Named fun (EEP-37): stacktrace preserves the name ---
named_fun_error() ->
    F = fun MyFun(0) -> error(named_boom);
            MyFun(N) -> MyFun(N - 1)
        end,
    {error, named_boom, Stk} = get_error(F, [3]),
    true = length(Stk) > 0,
    %% The top frame should be for the named fun
    {_, _, _, _} = hd(Stk),
    ok.

%% --- 7. Fun reference (fun M:F/A) error gets proper stacktrace ---
fun_ref_error() ->
    F = fun erlang:error/1,
    Stk = try
        F(ref_boom)
    catch
        error:ref_boom:S -> S
    end,
    true = is_list(Stk),
    true = length(Stk) > 0,
    ok.

%% --- 8. Old-style catch with fun error: destructure {_, Stack} shape ---
%% This tests compatibility with the OTP pattern:
%%   {'EXIT', {Reason, Stacktrace}} = (catch F())
catch_expr_fun_error() ->
    F = fun() -> error(catch_boom) end,
    Result = catch F(),
    {'EXIT', {catch_boom, Stk}} = Result,
    true = is_list(Stk),
    %% Stacktrace may be non-empty; if so, verify frame shape
    case Stk of
        [{_, _, _, _} | _] -> ok;
        [] -> ok
    end,
    ok.

%% Helper: call F with Args inside try, return {Class, Reason, Stacktrace}
get_error(F, Args) ->
    try
        apply(F, Args),
        no_error
    catch
        Class:Reason:Stk -> {Class, Reason, Stk}
    end.

id(I) -> I.
