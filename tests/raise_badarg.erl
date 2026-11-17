%% EXPECT: raise_badarg_ok
%% Intent: raise/3 returns badarg, without raising, for an invalid class or
%% non-list stacktrace, as on BEAM. Cleanup must keep the original exception.
%% Run this same fixture natively and on BEAM; frame-format validation is separate.
-module(raise_badarg).
-export([test/0]).

test() ->
    Mod = id(erlang),
    Name = id(raise),
    Fun = fun erlang:raise/3,
    Calls = [
        {qualified, fun(C, R, S) -> erlang:raise(C, R, S) end},
        {tail_call, fun raise_in_tail/3},
        {apply, fun(C, R, S) -> apply(erlang, raise, [C, R, S]) end},
        {computed, fun(C, R, S) -> Mod:Name(C, R, S) end},
        {remote_fun, Fun},
        {apply_fun, fun(C, R, S) -> apply(Fun, [C, R, S]) end}
    ],
    Stack = saved_stack(),
    [Frame | _] = Stack,
    BadClasses = [bogus, 42, [], {error}, <<"error">>],
    BadStacks = [not_a_list, 42, {}, #{}, <<>>, [Frame | not_a_list]],
    Invalid = [{C, S} || C <- BadClasses, S <- [[], Stack, not_a_list]] ++
              [{C, S} || C <- [error, throw, exit], S <- BadStacks],
    lists:foreach(fun({Call, F}) ->
        lists:foreach(fun({C, S}) ->
            {Call, C, S, {returned, badarg}} =
                {Call, C, S, outcome(fun() -> F(C, z, S) end)},
            %% A rejected call returns normally and reaches the next expression.
            {returned, continued} = outcome(fun() ->
                badarg = F(C, z, S), self() ! continued end),
            [continued] = drain(),
            badarg = (catch F(C, z, S)),
            %% Cleanup's returned badarg cannot replace a held exception.
            lists:foreach(fun(Class) ->
                {Class, a} = outcome(fun() ->
                    try original(Class) after self() ! F(C, z, S) end end),
                [badarg] = drain()
            end, [error, throw, exit]),
            {returned, body} = outcome(fun() ->
                try id(body) after self() ! F(C, z, S) end end),
            [badarg] = drain()
        end, Invalid),
        %% A valid empty stack still raises, even after rejected calls.
        lists:foreach(fun(Class) ->
            {Class, z} = outcome(fun() -> F(Class, z, []) end)
        end, [error, throw, exit])
    end, Calls),
    io:format("raise_badarg_ok~n"),
    ok.

raise_in_tail(Class, Reason, Stack) -> erlang:raise(Class, Reason, Stack).

saved_stack() -> try error(z) catch error:z:Stack -> Stack end.

original(error) -> error(a);
original(throw) -> throw(a);
original(exit) -> exit(a).

outcome(F) -> try {returned, F()} catch C:R -> {C, R} end.

drain() -> receive M -> [M | drain()] after 0 -> [] end.

id(X) -> X.
