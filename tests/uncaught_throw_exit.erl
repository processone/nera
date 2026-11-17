%% EXPECT: uncaught_throw_exit_ok
%% An uncaught throw ends its process with {{nocatch, Value}, Stacktrace},
%% as on BEAM, so monitors and links can tell it from an exit/1 reason.
%% Caught throws must keep ending the process normally.
-module(uncaught_throw_exit).
-export([test/0, fail/1]).

test() ->
    Old = process_flag(trap_exit, true),
    try
        %% Closure and MFA spawn entry points both end through a throw.
        stop = monitored(fun() -> throw(id(stop)) end),
        Payload = {detail, lists:seq(1, 40), <<"heap-owned value">>},
        Payload = monitored(fun() -> throw(Payload) end),
        {Pid, Ref} = spawn_monitor(?MODULE, fail, [mfa_throw]),
        {{nocatch, mfa_throw}, MfaStack} = down(Pid, Ref),
        ok = stack(MfaStack),
        true = lists:any(fun({M, F, A, _}) ->
            M =:= ?MODULE andalso F =:= fail andalso A =:= 1
        end, MfaStack),
        %% Spawning erlang:throw/1 itself; BEAM gives it an empty trace.
        {Bif, BifRef} = spawn_monitor(erlang, throw, [bif_throw]),
        {{nocatch, bif_throw}, BifStack} = down(Bif, BifRef),
        true = is_list(BifStack),

        %% Thrown values that are also special exit reasons stay wrapped.
        normal = monitored(fun() -> throw(id(normal)) end),
        kill = monitored(fun() -> throw(id(kill)) end),

        %% A throw from a handler, or one no catch clause matches, is
        %% uncaught too; cleanup in after still runs first.
        {again, first} = monitored(fun() ->
            try throw(id(first)) catch throw:R -> throw({again, R}) end
        end),
        unmatched = monitored(fun() ->
            try throw(id(unmatched)) catch error:_ -> wrong end
        end),
        Parent = self(),
        in_try = monitored(fun() ->
            try throw(id(in_try)) after Parent ! cleaned end
        end),
        receive cleaned -> ok after 0 -> error(missing_cleanup) end,

        %% Monitor a live, waiting child, then let it throw after resuming.
        Waiting = spawn(fun() -> receive go -> fail(resumed_throw) end end),
        WaitingRef = monitor(process, Waiting),
        Waiting ! go,
        {{nocatch, resumed_throw}, WaitingStack} = down(Waiting, WaitingRef),
        ok = stack(WaitingStack),

        %% The same child supplies identical DOWN and trapped EXIT reasons.
        Linked = spawn_link(fun() -> receive go -> fail(linked_throw) end end),
        LinkedRef = monitor(process, Linked),
        Linked ! go,
        LinkedReason = down(Linked, LinkedRef),
        {{nocatch, linked_throw}, LinkedStack} = LinkedReason,
        ok = stack(LinkedStack),
        LinkedReason = exited(Linked),

        %% A thrown normal or kill is not that exit reason: a non-trapping
        %% linked process dies with the same wrapped reason, neither carrying
        %% on nor being killed.
        lists:foreach(fun(Value) ->
            {Relay, RelayRef} = spawn_monitor(fun() ->
                Child = spawn_link(fun() -> receive go -> throw(Value) end end),
                Parent ! {child, self(), Child},
                receive go -> Child ! go end,
                receive never -> ok end
            end),
            Child = receive {child, Relay, C} -> C end,
            ChildRef = monitor(process, Child),
            Relay ! go,
            CascadeReason = down(Child, ChildRef),
            {{nocatch, Value}, CascadeStack} = CascadeReason,
            ok = stack(CascadeStack),
            CascadeReason = down(Relay, RelayRef)
        end, [normal, kill]),

        %% Re-raising a caught throw uses its saved trace; an explicitly
        %% empty trace stays empty.
        {Raised, RaisedRef} = spawn_monitor(fun() ->
            try fail(reraised_throw)
            catch throw:R:S ->
                Parent ! {saved_stack, self(), S},
                erlang:raise(throw, R, S)
            end
        end),
        Saved = receive {saved_stack, Raised, S} -> S end,
        {{nocatch, reraised_throw}, Saved} = down(Raised, RaisedRef),
        {Empty, EmptyRef} = spawn_monitor(fun() -> erlang:raise(throw, foo, []) end),
        {{nocatch, foo}, []} = down(Empty, EmptyRef),

        %% Caught throws still end the process normally.
        normal = ended(fun() -> catch throw(id(x)) end),
        normal = ended(fun() -> try throw(id(x)) catch throw:x -> ok end end),
        normal = ended(fun() -> try fail(x) catch _:_ -> ok end end),
        io:format("uncaught_throw_exit_ok~n"),
        ok
    after
        process_flag(trap_exit, Old)
    end.

monitored(F) ->
    {Pid, Ref} = spawn_monitor(F),
    {{nocatch, Value}, Stack} = down(Pid, Ref),
    ok = stack(Stack),
    Value.

ended(F) ->
    {Pid, Ref} = spawn_monitor(F),
    down(Pid, Ref).

stack([_ | _] = Stack) ->
    lists:foreach(fun({M, F, A, Info}) ->
        true = is_atom(M),
        true = is_atom(F),
        true = is_integer(A) orelse is_list(A),
        true = is_list(Info)
    end, Stack),
    ok.

down(Pid, Ref) ->
    receive {'DOWN', Ref, process, Pid, Reason} -> Reason
    after 5000 -> error(missing_down)
    end.

exited(Pid) ->
    receive {'EXIT', Pid, Reason} -> Reason
    after 5000 -> error(missing_exit)
    end.

fail(Value) -> throw(Value).
id(X) -> X.
