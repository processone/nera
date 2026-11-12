%% NERA-102: OTP accepts parenthesized module and function expressions.
%% EXPECT: parenthesized remote calls ok
-module(parenthesized_remote_calls).
-export([test/0, value/0, make_fun/0]).

test() ->
    M = id(?MODULE),
    F = id(value),
    check(function, ok, ?MODULE:(F)()),
    check(module, ok, (M):value()),
    check(both, ok, (M):(F)()),
    check(nested, ok, ((M)):((F))()),
    check(computed_module, ok, (id(?MODULE)):value()),
    check(computed_function, ok, ?MODULE:(id(value))()),
    check(computed_both, 3, (id(erlang)):(id(abs))(-3)),
    check(quoted_atoms, 5, ('erlang'):('abs')(-5)),
    check(arguments, <<1, 2>>, (id(binary)):(id(part))(<<0, 1, 2, 3>>, 1, 2)),
    check(expression, 6, (M):(begin value end)() =:= ok andalso 6),
    check(tail_call, 7, invoke(id(erlang), id(abs), -7)),
    check(returned_fun, 9, (M):(id(make_fun))()(9)),
    check(static_control, 4, erlang:abs(-4)),
    check(dynamic_control, ok, M:F()),
    check(fun_control, 8, (fun(X) -> X end)(8)),
    check(binary_size, <<3>>, <<(3):(8)>>),
    %% Review F1: module, function/callee, then arguments, including failure.
    check_order(remote_tail, {{ok, 1}, [module, function, arg]}, fun remote_tail/0),
    check_order(remote_args, {{ok, [a, b]}, [module, function, arg1, arg2]},
        fun() -> (mark(module, lists)):(mark(function, append))(mark(arg1, [a]), mark(arg2, [b])) end),
    check_order(remote_module_error, {{error, module}, [module]},
        fun() -> (fail(module)):(mark(function, abs))(fail(arg)) end),
    check_order(remote_function_error, {{error, function}, [module, function]},
        fun() -> (mark(module, erlang)):(fail(function))(fail(arg)) end),
    check_order(remote_argument_error, {{error, arg1}, [module, function, arg1]},
        fun() -> (mark(module, lists)):(mark(function, append))(fail(arg1), fail(arg2)) end),
    check_order(callee, {{ok, 1}, [callee, arg]},
        fun() -> (mark(callee, fun(X) -> X end))(mark(arg, 1)) end),
    check_order(callee_error, {{error, callee}, [callee]},
        fun() -> (fail(callee))(fail(arg)) end),
    check_order(callee_argument_error, {{error, arg1}, [callee, arg1]},
        fun() -> (mark(callee, fun(X, Y) -> {X, Y} end))(fail(arg1), fail(arg2)) end),
    check_order(same_function_try, {{ok, {caught, module}}, [module]}, fun same_function_try/0),
    check_order(same_function_callee_try, {{ok, {caught, callee}}, [callee]}, fun same_function_callee_try/0),
    check_order(remote_try, {{ok, 1}, [module, function, arg]}, fun remote_try/0),
    io:format("parenthesized remote calls ok~n"),
    ok.

value() -> ok.
make_fun() -> fun(X) -> X end.
invoke(M, F, X) -> (M):(F)(X).
id(X) -> X.

check(Label, Expected, Actual) ->
    Expected = Actual,
    io:format("~p: ~p~n", [Label, Actual]).

remote_tail() -> (mark(module, erlang)):(mark(function, abs))(mark(arg, -1)).
remote_try() ->
    try (mark(module, erlang)):(mark(function, abs))(mark(arg, -1))
    catch error:Reason -> {caught, Reason}
    end.
same_function_try() ->
    try (fail(module)):(fail(function))(fail(arg))
    catch error:Reason -> {caught, Reason}
    end.
same_function_callee_try() ->
    try (fail(callee))(fail(arg))
    catch error:Reason -> {caught, Reason}
    end.
check_order(Label, Expected, Fun) ->
    put(trace, []),
    Result = try Fun() of Value -> {ok, Value} catch error:Reason -> {error, Reason} end,
    check(Label, Expected, {Result, get(trace)}),
    erase(trace).
mark(Tag, Value) -> put(trace, get(trace) ++ [Tag]), Value.
fail(Tag) -> mark(Tag, Tag), erlang:error(Tag).
