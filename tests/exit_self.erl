%% EXPECT: exit_self_ok
%% A fatal exit(self(), Reason) stops before the next expression, without
%% entering catch handlers or after bodies. Run unchanged natively and on BEAM.
-module(exit_self).
-export([test/0]).

test() ->
    Reasons = [kill, normal, shutdown, killed, {shutdown, [worker, 42]},
               17, <<"stopped">>],
    Calls = [direct, qualified, apply_list, apply_fun, computed, fun_ref,
             tail_helper, caught, guarded, resumed],
    lists:foreach(fun(Reason) ->
        lists:foreach(fun(Call) ->
            ok = fatal(false, Reason, Call)
        end, Calls)
    end, Reasons),
    %% kill is untrappable, unlike an exit/1 exception with reason kill.
    lists:foreach(fun(Call) -> ok = fatal(true, kill, Call) end, Calls),
    lists:foreach(fun trapped/1, [normal, shutdown, killed,
                                 {shutdown, [worker, 42]}, 17, <<"stopped">>]),
    ok = external_normal(),
    ok = pending_trapped_normal(),
    lists:foreach(fun(Source) ->
        ok = mailbox_control(Source, kill),
        ok = mailbox_control(Source, external_normal)
    end, [literal, variable, trapped]),
    io:format("exit_self_ok~n"),
    ok.

fatal(Trap, Reason, Call) ->
    Parent = self(),
    {Pid, Ref} = spawn_monitor(fun() ->
        process_flag(trap_exit, Trap),
        signal(Call, Reason, Parent),
        effect(Parent, next_expression),
        error(later_error)
    end),
    Expected = case Reason of kill -> killed; _ -> Reason end,
    await_exit(Pid, Ref, Expected).

signal(direct, Reason, Parent) ->
    exit(self(), Reason),
    effect(Parent, direct_next);
signal(qualified, Reason, _Parent) ->
    erlang:exit(self(), Reason);
signal(apply_list, Reason, _Parent) ->
    apply(erlang, exit, id([self(), Reason]));
signal(apply_fun, Reason, _Parent) ->
    apply(fun erlang:exit/2, id([self(), Reason]));
signal(computed, Reason, _Parent) ->
    Module = id(erlang),
    Function = id(exit),
    Module:Function(self(), Reason);
signal(fun_ref, Reason, _Parent) ->
    Fun = id(fun exit/2),
    Fun(self(), Reason);
signal(tail_helper, Reason, Parent) ->
    tail_exit(Reason),
    effect(Parent, caller);
signal(caught, Reason, Parent) ->
    _ = (catch exit(self(), Reason)),
    effect(Parent, catch_return);
signal(guarded, Reason, Parent) ->
    try
        exit(self(), Reason),
        effect(Parent, try_body)
    catch
        _:_ -> effect(Parent, catch_handler)
    after
        effect(Parent, after_body)
    end;
signal(resumed, Reason, Parent) ->
    try resumed_exit(Reason, Parent)
    catch _:_ -> effect(Parent, resumed_handler)
    after effect(Parent, resumed_after)
    end.

tail_exit(Reason) ->
    exit(self(), Reason).

resumed_exit(Reason, Parent) ->
    Parent ! {ready, self()},
    receive go -> exit(self(), Reason) end.

effect(Parent, Where) ->
    Parent ! {unexpected, self(), Where}.

await_exit(Pid, Ref, Expected) ->
    receive
        {ready, Pid} ->
            Pid ! go,
            await_exit(Pid, Ref, Expected);
        {unexpected, Pid, Where} ->
            error({continued_after_self_exit, Where});
        {'DOWN', Ref, process, Pid, Actual} ->
            Expected = Actual,
            receive
                {unexpected, Pid, Where} ->
                    error({continued_after_self_exit, Where})
            after 0 -> ok
            end
    after 5000 -> error({self_exit_timeout, Expected})
    end.

%% Non-kill signals remain trappable messages, including normal and killed.
trapped(Reason) ->
    Parent = self(),
    {Pid, Ref} = spawn_monitor(fun() ->
        false = process_flag(trap_exit, true),
        Self = self(),
        Self ! before_signal,
        true = exit(Self, Reason),
        receive before_signal -> ok after 5000 -> error(missing_before) end,
        receive
            {'EXIT', Self, Reason} -> ok
        after 5000 -> error(missing_trapped_exit)
        end,
        true = process_flag(trap_exit, false),
        Parent ! {trapped, Self}
    end),
    receive
        {trapped, Pid} -> ok;
        {'DOWN', Ref, process, Pid, Why} -> error({trapped_self_exit, Why})
    after 5000 -> error(trapped_timeout)
    end,
    await_exit(Pid, Ref, normal).

%% Unlike self-sent normal, normal from another process is ignored.
external_normal() ->
    Parent = self(),
    {Pid, Ref} = spawn_monitor(fun() ->
        Parent ! {ready, self()},
        receive continue -> Parent ! {survived_normal, self()} end
    end),
    receive {ready, Pid} -> ok after 5000 -> error(ready_timeout) end,
    true = exit(Pid, normal),
    Pid ! continue,
    receive
        {survived_normal, Pid} -> ok;
        {'DOWN', Ref, process, Pid, Why} -> error({external_normal_exit, Why})
    after 5000 -> error(external_normal_timeout)
    end,
    await_exit(Pid, Ref, normal).

%% A normal self-signal sent while trapping is not a suicide request. Turning
%% trapping off before the pending signal is handled must still let us resume.
pending_trapped_normal() ->
    Parent = self(),
    {Pid, Ref} = spawn_monitor(fun() ->
        false = process_flag(trap_exit, true),
        true = exit(self(), normal),
        true = process_flag(trap_exit, false),
        Parent ! {ready, self()},
        receive
            go -> Parent ! {survived, self()}
        after 5000 -> error(pending_normal_timeout)
        end
    end),
    receive {ready, Pid} -> Pid ! go after 5000 -> error(ready_timeout) end,
    receive
        {survived, Pid} -> await_exit(Pid, Ref, normal);
        {'DOWN', Ref, process, Pid, Reason} -> error({pending_self_normal_exited, Reason})
    after 5000 -> error(pending_normal_timeout)
    end.

%% An ordinary or previously trapped EXIT tuple is never a pending signal.
%% Check both fused tuple sends and sends of an existing tuple. The trapped
%% case leaves the delivered message queued while a selective receive waits.
mailbox_control(Source, Next) ->
    Parent = self(),
    {Pid, Ref} = spawn_monitor(fun() ->
        Self = self(),
        case Source of
            literal -> Self ! {'EXIT', Self, normal};
            variable ->
                Message = id({'EXIT', Self, normal}),
                Self ! Message;
            trapped ->
                false = process_flag(trap_exit, true),
                true = exit(Self, normal),
                Parent ! {ready, Self},
                receive go -> ok after 5000 -> error(trap_barrier_timeout) end,
                true = process_flag(trap_exit, false)
        end,
        case Next of
            kill ->
                exit(Self, kill),
                effect(Parent, mailbox_after_kill);
            external_normal ->
                Parent ! {normal_ready, Self},
                receive go -> ok after 5000 -> error(normal_barrier_timeout) end,
                receive
                    {'EXIT', Self, normal} -> Parent ! {checked, Self}
                after 5000 -> error(missing_ordinary_exit_message)
                end
        end
    end),
    case Next of
        kill -> await_exit(Pid, Ref, killed);
        external_normal -> await_checked(Pid, Ref)
    end.

await_checked(Pid, Ref) ->
    receive
        {ready, Pid} ->
            Pid ! go,
            await_checked(Pid, Ref);
        {normal_ready, Pid} ->
            true = exit(Pid, normal),
            Pid ! go,
            await_checked(Pid, Ref);
        {checked, Pid} -> await_exit(Pid, Ref, normal);
        {'DOWN', Ref, process, Pid, Why} -> error({ordinary_exit_became_signal, Why})
    after 5000 -> error(mailbox_control_timeout)
    end.

id(Value) -> Value.
