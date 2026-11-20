%% EXPECT: apply_improper_args_ok
%% apply(Module, Function, Args) and apply(Fun, Args) raise error:badarg when
%% Args is not a proper list: a term that is not a list, or a list with an
%% improper tail. The function is not called. This holds for built-in and
%% compiled functions, unknown functions, computed names, tail calls, a nested
%% apply, remote funs, fun erlang:apply/2,3, a term that is not a fun and a
%% process spawned to run apply. A proper list still calls the function. Run
%% this same fixture on native Nera and BEAM.
-module(apply_improper_args).
-export([test/0, zero/0, one/1, note/1, note0/0]).

test() ->
    Self = self(),
    F0 = fun() -> Self ! {called, fun0}, zero end,
    F1 = fun(X) -> Self ! {called, fun1}, X end,
    Mod = id(erlang),
    Apply2 = fun erlang:apply/2,
    Apply3 = fun erlang:apply/3,
    Badarg = [
        {bif_improper, fun() -> apply(erlang, length, [[1, 2] | id(z)]) end},
        {bif_not_a_list, fun() -> apply(erlang, node, id(not_a_list)) end},
        {bif_binary, fun() -> apply(erlang, self, id(<<>>)) end},
        {error_2_improper, fun() -> apply(erlang, error, [x, [a] | id(z)]) end},
        {qualified, fun() -> erlang:apply(erlang, length, [[1] | id(z)]) end},
        {compiled_improper, fun() -> apply(?MODULE, note, [a | id(z)]) end},
        {compiled_not_a_list, fun() -> apply(?MODULE, note0, id(not_a_list)) end},
        {compiled_two_heads, fun() -> apply(?MODULE, note, [a, b | id(z)]) end},
        {unknown_function, fun() -> apply(no_such_module, no_such_function, [a | id(z)]) end},
        {computed, fun() -> apply(Mod, id(length), [[1] | id(z)]) end},
        {tail_call, fun() -> tail_apply(erlang, length, [[1] | id(z)]) end},
        {nested_inner, fun() -> apply(erlang, apply, [erlang, length, [[1] | id(z)]]) end},
        {nested_outer, fun() -> apply(erlang, apply, [erlang, length | id(z)]) end},
        {fun_improper, fun() -> apply(F1, [1 | id(z)]) end},
        {fun_not_a_list, fun() -> apply(F0, id(not_a_list)) end},
        {fun_wrong_arity, fun() -> apply(F1, [1, 2 | id(z)]) end},
        {remote_fun, fun() -> apply(fun ?MODULE:note/1, [a | id(z)]) end},
        {bif_fun, fun() -> apply(fun erlang:length/1, [[1] | id(z)]) end},
        {not_a_fun, fun() -> apply(id(not_a_fun), [a | id(z)]) end},
        {fun_tail_call, fun() -> tail_apply(F1, [1 | id(z)]) end},
        {nested_fun, fun() -> apply(erlang, apply, [F1, [1 | id(z)]]) end},
        {apply_3_as_fun, fun() -> Apply3(erlang, length, [[1] | id(z)]) end},
        {apply_2_as_fun, fun() -> Apply2(F1, [1 | id(z)]) end}
    ],
    [{Name, {error, badarg}} = {Name, outcome(F)} || {Name, F} <- Badarg],

    %% None of those calls reached the function.
    [] = drain(),

    %% A process whose last call is apply, and a process spawned to run
    %% erlang:apply/3, exit with badarg.
    Spawned = [
        {spawned_tail_call, spawn_monitor(fun() -> tail_apply(erlang, length, [[1] | id(z)]) end)},
        {spawned_apply, spawn_monitor(erlang, apply, [erlang, length, [[1] | id(z)]])}
    ],
    [{Name, badarg} = {Name, down_reason(PidRef)} || {Name, PidRef} <- Spawned],

    Proper = [
        {bif, 2, fun() -> apply(erlang, length, id([[1, 2]])) end},
        {bif_no_args, true, fun() -> apply(erlang, self, id([])) =:= Self end},
        {compiled, {one, a}, fun() -> apply(?MODULE, one, id([a])) end},
        {compiled_no_args, zero, fun() -> apply(?MODULE, zero, id([])) end},
        {computed, 1, fun() -> apply(Mod, id(length), id([[a]])) end},
        {tail_call, 3, fun() -> tail_apply(erlang, length, id([[a, b, c]])) end},
        {nested, 3, fun() -> apply(erlang, apply, [erlang, length, id([[a, b, c]])]) end},
        {fun_one, 1, fun() -> apply(F1, id([1])) end},
        {fun_zero, zero, fun() -> apply(F0, id([])) end},
        {fun_tail_call, 1, fun() -> tail_apply(F1, id([1])) end}
    ],
    [{Name, {ok, Value}} = {Name, outcome(F)} || {Name, Value, F} <- Proper],

    %% The funs given a proper list ran, in order.
    [{called, fun1}, {called, fun0}, {called, fun1}] = drain(),

    io:format("apply_improper_args_ok~n"),
    ok.

tail_apply(M, F, Args) -> apply(M, F, Args).

tail_apply(Fun, Args) -> apply(Fun, Args).

zero() -> zero.

one(X) -> {one, X}.

note(X) -> self() ! {called, note, X}.

note0() -> self() ! {called, note0}.

down_reason({Pid, Ref}) ->
    receive
        {'DOWN', Ref, process, Pid, {Reason, Stack}} when is_list(Stack) -> Reason
    after 5000 -> timeout
    end.

outcome(F) ->
    try {ok, F()} catch Class:Reason -> {Class, Reason} end.

drain() -> receive M -> [M | drain()] after 0 -> [] end.

id(X) -> X.
