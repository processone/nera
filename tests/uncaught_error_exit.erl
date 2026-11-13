%% EXPECT: uncaught_error_exit_ok
%% Uncaught error exits carry {Reason, Stacktrace} to monitors and links,
%% as on BEAM. Explicit exits and caught errors must keep their own shape.
-module(uncaught_error_exit).
-export([test/0, fail/1]).

test() ->
    Old = process_flag(trap_exit, true),
    try
        %% Closure and MFA spawn entry points both terminate through errors.
        foo = monitored(fun() -> error(id(foo)) end),
        Payload = {detail, lists:seq(1, 40), <<"heap-owned reason">>},
        Payload = monitored(fun() -> error(Payload) end),
        {Pid, Ref} = spawn_monitor(?MODULE, fail, [mfa_error]),
        {mfa_error, MfaStack} = down(Pid, Ref),
        ok = stack(MfaStack),
        true = lists:any(fun({M, F, A, _}) ->
            M =:= ?MODULE andalso F =:= fail andalso A =:= 1
        end, MfaStack),

        %% Runtime errors use the same envelope, even if Reason is a tuple.
        {badmatch, wrong} = monitored(fun() -> right = id(wrong) end),
        badarg = monitored(fun() -> hd(id(not_a_list)) end),
        normal = monitored(fun() -> error(id(normal)) end),
        kill = monitored(fun() -> error(id(kill)) end),

        %% Monitor a live, waiting child, then let it fail after resuming.
        Waiting = spawn(fun() -> receive go -> fail(resumed_error) end end),
        WaitingRef = monitor(process, Waiting),
        Waiting ! go,
        {resumed_error, WaitingStack} = down(Waiting, WaitingRef),
        ok = stack(WaitingStack),

        %% The same child supplies identical DOWN and trapped EXIT reasons.
        Linked = spawn_link(fun() -> receive go -> fail(linked_error) end end),
        LinkedRef = monitor(process, Linked),
        Linked ! go,
        LinkedReason = down(Linked, LinkedRef),
        {linked_error, LinkedStack} = LinkedReason,
        ok = stack(LinkedStack),
        LinkedReason = exited(Linked),

        %% A non-trapping linked process propagates the original envelope,
        %% rather than wrapping it a second time or substituting its own stack.
        Parent = self(),
        {Relay, RelayRef} = spawn_monitor(fun() ->
            Child = spawn_link(fun() -> receive go -> fail(cascade_error) end end),
            Parent ! {child, self(), Child},
            receive go -> Child ! go end,
            receive never -> ok end
        end),
        Child = receive {child, Relay, C} -> C end,
        ChildRef = monitor(process, Child),
        Relay ! go,
        CascadeReason = down(Child, ChildRef),
        {cascade_error, CascadeStack} = CascadeReason,
        ok = stack(CascadeStack),
        CascadeReason = down(Relay, RelayRef),

        %% Re-raising a caught error must use its saved trace, not the
        %% scheduler's current stack; a deliberately empty trace stays empty.
        {Raised, RaisedRef} = spawn_monitor(fun() ->
            try fail(reraised_error)
            catch error:R:S ->
                Parent ! {saved_stack, self(), S},
                erlang:raise(error, R, S)
            end
        end),
        Saved = receive {saved_stack, Raised, S} -> S end,
        {reraised_error, Saved} = down(Raised, RaisedRef),
        {Empty, EmptyRef} = spawn_monitor(fun() -> erlang:raise(error, foo, []) end),
        {foo, []} = down(Empty, EmptyRef),

        %% Only uncaught error-class exceptions acquire the envelope.
        lists:foreach(fun(R) ->
            {ExitPid, ExitRef} = spawn_opt(fun() -> exit(R) end, [link, monitor]),
            R = down(ExitPid, ExitRef),
            R = exited(ExitPid)
        end, [normal, explicit_exit, {foo, []}]),
        {Normal, NormalRef} = spawn_monitor(fun() -> ok end),
        normal = down(Normal, NormalRef),
        {Caught, CaughtRef} = spawn_monitor(fun() ->
            try fail(caught_error) catch error:caught_error -> ok end
        end),
        normal = down(Caught, CaughtRef),
        {Signalled, SignalledRef} = spawn_monitor(fun() -> receive never -> ok end end),
        true = exit(Signalled, {external, []}),
        {external, []} = down(Signalled, SignalledRef),
        {Killed, KilledRef} = spawn_monitor(fun() -> receive never -> ok end end),
        true = exit(Killed, kill),
        killed = down(Killed, KilledRef),
        io:format("uncaught_error_exit_ok~n"),
        ok
    after
        process_flag(trap_exit, Old)
    end.

monitored(F) ->
    {Pid, Ref} = spawn_monitor(F),
    {Reason, Stack} = down(Pid, Ref),
    ok = stack(Stack),
    Reason.

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

fail(Reason) -> error(Reason).
id(X) -> X.
