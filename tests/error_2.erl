%% EXPECT: error_2_ok
%% error(Reason, Args) and erlang:error(Reason, Args) raise error:Reason,
%% as on BEAM, through every way of calling them: the auto-imported and
%% qualified names, a tail call, apply/3, a fun and a computed module. Args
%% does not change the class or the reason, and the code after the call does
%% not run. Run this same fixture on native Nera and BEAM.
-module(error_2).
-export([test/0]).

test() ->
    Mod = id(erlang),
    Fun = fun erlang:error/2,
    Cases = [
        {auto_import, fun() -> error(x, [a]) end},
        {qualified, fun() -> erlang:error(x, [a]) end},
        {args_none, fun() -> error(x, none) end},
        {args_empty, fun() -> error(x, []) end},
        {args_not_a_list, fun() -> error(x, id(not_a_list)) end},
        {tail_call, fun() -> raise_in_tail(x) end},
        {apply, fun() -> apply(erlang, error, [x, [a]]) end},
        {apply_computed, fun() -> apply(Mod, id(error), [x, [a]]) end},
        {remote_fun, fun() -> Fun(x, [a]) end},
        {computed_module, fun() -> Mod:error(x, [a]) end}
    ],
    [{Name, {error, x}} = {Name, outcome(F)} || {Name, F} <- Cases],

    %% A reason built at run time is kept whole.
    {error, {badarg, [1, 2]}} = outcome(fun() -> error({badarg, id([1, 2])}, [id(1)]) end),

    %% The stacktrace is a list, as for error/1.
    true = try error(x, [a]) catch error:x:Stack -> is_list(Stack) end,

    %% The code after the call does not run.
    {error, x} = outcome(fun() -> error(x, [a]), self() ! after_error end),
    [] = drain(),

    %% catch gives {'EXIT', {Reason, Stack}}.
    {'EXIT', {x, CatchStack}} = (catch error(x, [a])),
    true = is_list(CatchStack),

    io:format("error_2_ok~n"),
    ok.

raise_in_tail(Reason) -> erlang:error(Reason, [Reason]).

outcome(F) ->
    try {ok, F()} catch Class:Reason -> {Class, Reason} end.

drain() -> receive M -> [M | drain()] after 0 -> [] end.

id(X) -> X.
