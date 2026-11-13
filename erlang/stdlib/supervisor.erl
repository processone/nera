%% Minimal supervisor implementation for nera.
%%
%% Supports:
%% - start_link/2, start_link/3 (with {local, Name} registration)
%% - one_for_one restart strategy
%% - Child spec format: {Id, {Mod, Fun, Args}, Restart, Shutdown, Type, Modules}
%% - Restart types: permanent, temporary, transient
%% - Monitors children and restarts on crash, trying a failed restart again
%% - MaxR/MaxT restart intensity limiting
%% - Start results as in OTP: a child that fails to start fails the start with
%%   {shutdown, {failed_to_start_child, Id, Reason}}, a child whose start
%%   returns ignore is kept as not running, and init/1 returning ignore makes
%%   start_link/2,3 return ignore
%% - Stopping as in OTP: the supervisor stops when its parent exits or sends
%%   it an exit signal, and before it exits it stops its children one by one,
%%   the last started first, by their shutdown setting
%%
%% Not supported: one_for_all, rest_for_one, simple_one_for_one,
%%                dynamic children, which_children, count_children.
%%
-module(supervisor).
-export([start_link/2, start_link/3]).

%% start_link/3 — start a named supervisor.
%% Example: supervisor:start_link({local, my_sup}, my_sup_mod, []).
start_link({local, Name}, Mod, Args) ->
    start_supervisor(Name, Mod, Args).

%% start_link/2 — start an anonymous supervisor.
start_link(Mod, Args) ->
    start_supervisor(none, Mod, Args).

%% A name that is already registered gives {error, {already_started, Pid}}.
%% Otherwise the caller monitors the new supervisor from the moment it is
%% spawned, so it gets an answer even when init/1 crashes and no
%% acknowledgement comes, as in OTP: {error, Reason} for a caller that traps
%% exits. A caller that does not trap exits exits with Reason through the link.
%% When init/1 returns ignore, the supervisor answers ignore and exits normally.
start_supervisor(MaybeName, Mod, Args) ->
    case whereis_name(MaybeName) of
        undefined -> spawn_supervisor(MaybeName, Mod, Args);
        Pid -> {error, {already_started, Pid}}
    end.

spawn_supervisor(MaybeName, Mod, Args) ->
    Parent = self(),
    Ref = make_ref(),
    {Pid, MRef} = spawn_opt(fun() ->
        init_it(Mod, Args, MaybeName, Parent, Ref)
    end, [link, monitor]),
    receive
        {sup_ack, Ref, ok} ->
            demonitor(MRef),
            receive {'DOWN', MRef, process, _, _} -> ok after 0 -> ok end,
            {ok, Pid};
        {sup_nack, Ref, Return} ->
            %% The supervisor exits normally right after this answer.
            receive {'DOWN', MRef, process, Pid, _} -> ok end,
            flush_exit(Pid),
            Return;
        {'DOWN', MRef, process, Pid, Reason} ->
            flush_exit(Pid),
            {error, Reason}
    end.

init_it(Mod, Args, MaybeName, Parent, Ref) ->
    case register_name(MaybeName) of
        true ->
            init_supervisor(Mod, Args, MaybeName, Parent, Ref);
        {false, Pid} ->
            Parent ! {sup_nack, Ref, {error, {already_started, Pid}}}
    end.

init_supervisor(Mod, Args, MaybeName, Parent, Ref) ->
    case init_mod(Mod, Args) of
        {ok, {ok, {SupFlags, ChildSpecs}}} ->
            start_supervision(Mod, SupFlags, ChildSpecs, MaybeName, Parent, Ref);
        {ok, ignore} ->
            unregister_name(MaybeName),
            Parent ! {sup_nack, Ref, ignore};
        {ok, Else} ->
            %% Any other value, {stop, Reason} included, as in OTP.
            init_failed(Mod, MaybeName, {bad_return, {Mod, init, Else}}, true);
        {thrown, Thrown} ->
            init_thrown(Mod, MaybeName, Parent, Ref, Thrown);
        {'EXIT', Class, Reason, Stacktrace} ->
            init_failed(Mod, MaybeName, exit_reason(Class, Reason, Stacktrace),
                        report(Class, Reason))
    end.

init_mod(Mod, Args) ->
    try
        {ok, Mod:init(Args)}
    catch
        throw:Thrown -> {thrown, Thrown};
        Class:Reason:Stacktrace -> {'EXIT', Class, Reason, Stacktrace}
    end.

%% In OTP a value thrown by init/1 escapes the supervisor module and is read
%% as the return value of a gen_server's init/1.
init_thrown(Mod, MaybeName, _Parent, _Ref, {stop, Reason}) ->
    init_failed(Mod, MaybeName, Reason, not is_shutdown(Reason));
init_thrown(_Mod, MaybeName, Parent, Ref, {error, _} = Error) ->
    unregister_name(MaybeName),
    Parent ! {sup_nack, Ref, Error};
init_thrown(_Mod, MaybeName, Parent, Ref, ignore) ->
    unregister_name(MaybeName),
    Parent ! {sup_nack, Ref, ignore};
init_thrown(Mod, MaybeName, _Parent, _Ref, Else) ->
    init_failed(Mod, MaybeName, {bad_return_value, Else}, true).

%% Read the flags and start the children. A child that fails to start fails
%% the start with {shutdown, {failed_to_start_child, Id, Reason}}, as in OTP,
%% once the children started before it have been stopped. An invalid child
%% spec fails it with {start_spec, Error} before any child starts, and a
%% crash in reading the flags fails it with its reason.
start_supervision(Mod, SupFlags, ChildSpecs, MaybeName, Parent, Ref) ->
    case start_children_safely(SupFlags, ChildSpecs) of
        {started, Strategy, MaxR, MaxT, Children} ->
            Parent ! {sup_ack, Ref, ok},
            supervisor_loop(Parent, Strategy, MaxR, MaxT, [], Children);
        {failed_to_start_child, Id, Reason, Started} ->
            terminate_children(Started),
            %% The child's failure is already logged; OTP does not report a
            %% shutdown as a crash.
            init_failed(Mod, MaybeName,
                        {shutdown, {failed_to_start_child, Id, Reason}}, false);
        {start_spec, _Error} = Reason ->
            init_failed(Mod, MaybeName, Reason, true);
        {'EXIT', Class, Reason, Stacktrace} ->
            init_failed(Mod, MaybeName, exit_reason(Class, Reason, Stacktrace),
                        report(Class, Reason))
    end.

start_children_safely(SupFlags, ChildSpecs) ->
    try
        process_flag(trap_exit, true),
        {Strategy, MaxR, MaxT} = parse_sup_flags(SupFlags),
        %% Check every child spec before starting any child, as OTP does,
        %% so an invalid one leaves no child running.
        case check_startspec(ChildSpecs, []) of
            ok ->
                case start_children(ChildSpecs, []) of
                    {ok, Children} -> {started, Strategy, MaxR, MaxT, Children};
                    {error, Id, Why, Started} -> {failed_to_start_child, Id, Why, Started}
                end;
            Error ->
                {start_spec, Error}
        end
    catch
        Class:Reason:Stacktrace -> {'EXIT', Class, Reason, Stacktrace}
    end.

%% Free the name before exiting, so the caller can start the supervisor again
%% as soon as it gets {error, Reason}.
init_failed(Mod, MaybeName, Reason, Report) ->
    case Report of
        true -> error_logger:error_msg("Supervisor ~p:init/1 failed: ~p~n", [Mod, Reason]);
        false -> ok
    end,
    unregister_name(MaybeName),
    exit(Reason).

%% Whether OTP logs a failure: always for an error or a throw, and for an
%% exit unless its reason is a shutdown.
report(exit, Reason) -> not is_shutdown(Reason);
report(_Class, _Reason) -> true.

%% Reasons OTP does not report as a crash.
is_shutdown(normal) -> true;
is_shutdown(shutdown) -> true;
is_shutdown({shutdown, _}) -> true;
is_shutdown(_) -> false.

%% The reason a process exits with when the exception escapes, as on BEAM.
exit_reason(error, Reason, Stacktrace) -> {Reason, Stacktrace};
exit_reason(exit, Reason, _Stacktrace) -> Reason;
exit_reason(throw, Reason, Stacktrace) -> {{nocatch, Reason}, Stacktrace}.

whereis_name(none) -> undefined;
whereis_name(Name) -> whereis(Name).

%% Another process may take the name between the caller's check and here.
register_name(none) -> true;
register_name(Name) ->
    try register(Name, self())
    catch error:badarg -> {false, whereis(Name)}
    end.

unregister_name(none) -> ok;
unregister_name(Name) ->
    try unregister(Name) catch error:badarg -> ok end,
    ok.

%% A caller that traps exits gets {'EXIT', Pid, Reason} through the link before
%% the 'DOWN' message; consume it, as OTP's proc_lib does.
flush_exit(Pid) ->
    unlink(Pid),
    receive {'EXIT', Pid, _} -> ok after 0 -> ok end.

parse_sup_flags({Strategy, MaxR, MaxT}) ->
    {Strategy, MaxR, MaxT};
parse_sup_flags(Strategy) when is_atom(Strategy) ->
    {Strategy, 5, 10}.

%% Check the child specs as OTP's supervisor does, with its errors. Returns
%% ok or the first error. Map child specs are not supported.
check_startspec([], _Ids) ->
    ok;
check_startspec([ChildSpec | Rest], Ids) ->
    case check_childspec(ChildSpec) of
        {ok, Id} ->
            case lists:member(Id, Ids) of
                true -> {duplicate_child_name, Id};
                false -> check_startspec(Rest, [Id | Ids])
            end;
        Error ->
            Error
    end.

check_childspec({Id, MFA, Restart, Shutdown, Type, Modules}) ->
    try
        valid_mfa(MFA),
        valid_restart_type(Restart),
        valid_child_type(Type),
        valid_shutdown(Shutdown),
        valid_modules(Modules),
        {ok, Id}
    catch
        throw:Error -> Error
    end;
check_childspec(ChildSpec) ->
    {invalid_child_spec, ChildSpec}.

valid_mfa({M, F, A}) when is_atom(M), is_atom(F), is_list(A) -> ok;
valid_mfa(MFA) -> throw({invalid_mfa, MFA}).

valid_restart_type(permanent) -> ok;
valid_restart_type(temporary) -> ok;
valid_restart_type(transient) -> ok;
valid_restart_type(Restart) -> throw({invalid_restart_type, Restart}).

valid_child_type(worker) -> ok;
valid_child_type(supervisor) -> ok;
valid_child_type(Type) -> throw({invalid_child_type, Type}).

valid_shutdown(Shutdown) when is_integer(Shutdown), Shutdown >= 0 -> ok;
valid_shutdown(infinity) -> ok;
valid_shutdown(brutal_kill) -> ok;
valid_shutdown(Shutdown) -> throw({invalid_shutdown, Shutdown}).

valid_modules(dynamic) -> ok;
valid_modules(Modules) when is_list(Modules) ->
    lists:foreach(fun(M) when is_atom(M) -> ok;
                     (M) -> throw({invalid_module, M})
                  end, Modules);
valid_modules(Modules) -> throw({invalid_modules, Modules}).

%% Start the children in order, stopping at the first that fails to start.
%% Returns {ok, [{Id, Pid, ChildSpec}]} or {error, Id, Reason, Started}, with
%% the children started before the failure, both in start order. A child
%% whose start returned ignore is kept with the pid undefined, unless it is
%% temporary, as in OTP.
start_children([], Acc) ->
    {ok, lists:reverse(Acc)};
start_children([ChildSpec | Rest], Acc) ->
    {Id, MFA, Restart, _Shutdown, _Type, _Modules} = ChildSpec,
    case do_start_child(MFA) of
        {ok, undefined} when Restart =:= temporary ->
            start_children(Rest, Acc);
        {ok, Pid} ->
            start_children(Rest, [{Id, Pid, ChildSpec} | Acc]);
        {error, Reason} ->
            error_logger:error_msg("Supervisor: failed to start ~p: ~p~n",
                                   [Id, Reason]),
            {error, Id, Reason, lists:reverse(Acc)}
    end.

%% Run a child's start function and read its result as OTP's supervisor does:
%% {ok, Pid}, {ok, undefined} for ignore, or {error, Reason}. A thrown value
%% is the start function's return value; an exception is
%% {error, {'EXIT', Reason}}; any other value is {error, Value}.
do_start_child({Mod, Fun, Args}) ->
    Result = try
                 apply(Mod, Fun, Args)
             catch
                 throw:Thrown -> Thrown;
                 exit:Reason -> {error, {'EXIT', Reason}};
                 error:Reason:Stacktrace -> {error, {'EXIT', {Reason, Stacktrace}}}
             end,
    start_result(Result).

start_result({ok, Pid}) when is_pid(Pid) -> {ok, Pid};
start_result({ok, Pid, _Info}) when is_pid(Pid) -> {ok, Pid};
start_result(ignore) -> {ok, undefined};
start_result({error, _Reason} = Error) -> Error;
start_result(Other) -> {error, Other}.

%% Main supervisor loop — trap_exit is on, so we receive EXIT messages.
%% Parent is the process that started the supervisor. RestartLog is a list of
%% timestamps (seconds) of recent restarts.
supervisor_loop(Parent, Strategy, MaxR, MaxT, RestartLog, Children) ->
    receive
        {'EXIT', Parent, Reason} ->
            %% As in OTP, the parent's exit, or an exit signal from it, stops
            %% the supervisor with that reason, after its children.
            stop_supervisor(Reason, Children);
        {'EXIT', Pid, Reason} ->
            case find_child_by_pid(Pid, Children) of
                {ok, {Id, _Pid, ChildSpec}} ->
                    {_Id, _MFA, Restart, _Shutdown, _Type, _Modules} = ChildSpec,
                    case should_restart(Restart, Reason) of
                        true ->
                            error_logger:info_msg("Supervisor: restarting ~p (reason: ~p)~n",
                                                  [Id, Reason]),
                            restart_child(Parent, Strategy, MaxR, MaxT, RestartLog,
                                          Id, ChildSpec, Children);
                        false ->
                            NewChildren = remove_child(Pid, Children),
                            supervisor_loop(Parent, Strategy, MaxR, MaxT, RestartLog, NewChildren)
                    end;
                none ->
                    %% Unknown child, ignore
                    supervisor_loop(Parent, Strategy, MaxR, MaxT, RestartLog, Children)
            end;
        {'$supervisor_try_again_restart', Id, Tag} ->
            case find_child_by_id(Id, Children) of
                {ok, {Id, {restarting, Tag}, ChildSpec}} ->
                    restart_child(Parent, Strategy, MaxR, MaxT, RestartLog,
                                  Id, ChildSpec, Children);
                _ ->
                    supervisor_loop(Parent, Strategy, MaxR, MaxT, RestartLog, Children)
            end;
        _Other ->
            supervisor_loop(Parent, Strategy, MaxR, MaxT, RestartLog, Children)
    end.

%% Keep only restart entries within the last MaxT seconds.
prune_restart_log([], _Now, _MaxT) -> [];
prune_restart_log([T | Rest], Now, MaxT) ->
    case (Now - T) =< MaxT of
        true -> [T | prune_restart_log(Rest, Now, MaxT)];
        false -> prune_restart_log(Rest, Now, MaxT)
    end.

should_restart(permanent, _Reason) -> true;
should_restart(temporary, _Reason) -> false;
should_restart(transient, normal) -> false;
should_restart(transient, shutdown) -> false;
should_restart(transient, {shutdown, _}) -> false;
should_restart(transient, _Reason) -> true.

find_child_by_id(_Id, []) -> none;
find_child_by_id(Id, [{Id, _Pid, _ChildSpec} = Child | _Rest]) -> {ok, Child};
find_child_by_id(Id, [_ | Rest]) -> find_child_by_id(Id, Rest).

find_child_by_pid(_Pid, []) -> none;
find_child_by_pid(Pid, [{Id, Pid, ChildSpec} | _Rest]) ->
    {ok, {Id, Pid, ChildSpec}};
find_child_by_pid(Pid, [_ | Rest]) ->
    find_child_by_pid(Pid, Rest).

remove_child(_Pid, []) -> [];
remove_child(Pid, [{_Id, Pid, _Spec} | Rest]) -> Rest;
remove_child(Pid, [Child | Rest]) ->
    [Child | remove_child(Pid, Rest)].

%% Restart a child, as OTP does. Each try counts against the restart limit,
%% and past it the supervisor stops its other children and exits with
%% shutdown. A try that fails marks the child restarting and is made again
%% once the messages already waiting have been handled; the tag makes only
%% that one message start the next try. A restart that returns ignore keeps
%% the child as not running.
restart_child(Parent, Strategy, MaxR, MaxT, RestartLog, Id, ChildSpec, Children) ->
    Now = erlang:system_time(second),
    NewLog = [Now | prune_restart_log(RestartLog, Now, MaxT)],
    case length(NewLog) > MaxR of
        true ->
            error_logger:error_msg(
                "Supervisor: max restart intensity reached (~p in ~p s), shutting down~n",
                [MaxR, MaxT]),
            terminate_children(lists:keydelete(Id, 1, Children)),
            exit(shutdown);
        false ->
            {_Id, MFA, _Restart, _Shutdown, _Type, _Modules} = ChildSpec,
            NewChildren = case do_start_child(MFA) of
                {ok, NewPid} ->
                    replace_child(Id, NewPid, ChildSpec, Children);
                {error, Reason} ->
                    error_logger:error_msg("Supervisor: restart of ~p failed: ~p~n",
                                           [Id, Reason]),
                    Tag = make_ref(),
                    self() ! {'$supervisor_try_again_restart', Id, Tag},
                    replace_child(Id, {restarting, Tag}, ChildSpec, Children)
            end,
            supervisor_loop(Parent, Strategy, MaxR, MaxT, NewLog, NewChildren)
    end.

replace_child(_Id, _NewPid, _ChildSpec, []) -> [];
replace_child(Id, NewPid, ChildSpec, [{Id, _OldPid, _OldSpec} | Rest]) ->
    [{Id, NewPid, ChildSpec} | Rest];
replace_child(Id, NewPid, ChildSpec, [Child | Rest]) ->
    [Child | replace_child(Id, NewPid, ChildSpec, Rest)].

%% Stop the children, then exit with Reason. OTP reports a reason that is not
%% a shutdown as a crash.
stop_supervisor(Reason, Children) ->
    terminate_children(Children),
    case is_shutdown(Reason) of
        true -> ok;
        false -> error_logger:error_msg("Supervisor ~p terminating: ~p~n", [self(), Reason])
    end,
    exit(Reason).

%% Stop the running children one by one, the last started first, as OTP
%% does. Children are in start order; one that is not running is skipped.
terminate_children(Children) ->
    lists:foreach(fun terminate_child/1, lists:reverse(Children)).

terminate_child({Id, Pid, ChildSpec}) when is_pid(Pid) ->
    {_Id, _MFA, Restart, Shutdown, _Type, _Modules} = ChildSpec,
    case shutdown(Pid, Shutdown, Restart) of
        ok ->
            ok;
        {error, Reason} ->
            error_logger:error_msg("Supervisor: ~p stopped with ~p when shut down~n",
                                   [Id, Reason])
    end;
terminate_child(_NotRunning) ->
    ok.

%% Stop a child by its shutdown setting, as OTP does: brutal_kill kills it;
%% a time in milliseconds, or infinity, sends it shutdown and kills it if it
%% is still running after that time. The supervisor watches the child with a
%% monitor, so a child that unlinked itself cannot make it wait forever.
%% Returns ok, or {error, Reason} when the child stopped with another reason
%% than expected.
shutdown(Pid, brutal_kill, Restart) ->
    MRef = monitor(process, Pid),
    exit(Pid, kill),
    receive
        {'DOWN', MRef, process, Pid, Reason} ->
            shutdown_result(unlink_flush(Pid, Reason), killed, Restart)
    end;
shutdown(Pid, Time, Restart) ->
    MRef = monitor(process, Pid),
    exit(Pid, shutdown),
    receive
        {'DOWN', MRef, process, Pid, Reason} ->
            shutdown_result(unlink_flush(Pid, Reason), shutdown, Restart)
    after Time ->
        kill_child(Pid, MRef, Restart)
    end.

%% Kept out of the after body above: native code does not yet run a receive
%% written directly in an after body correctly.
kill_child(Pid, MRef, Restart) ->
    exit(Pid, kill),
    receive
        {'DOWN', MRef, process, Pid, Reason} ->
            shutdown_result(unlink_flush(Pid, Reason), shutdown, Restart)
    end.

%% A child that is not permanent may also stop normally or with a shutdown.
shutdown_result(Expected, Expected, _Restart) -> ok;
shutdown_result(normal, _Expected, Restart) when Restart =/= permanent -> ok;
shutdown_result(shutdown, _Expected, Restart) when Restart =/= permanent -> ok;
shutdown_result({shutdown, _}, _Expected, Restart) when Restart =/= permanent -> ok;
shutdown_result(Reason, _Expected, _Restart) -> {error, Reason}.

%% The child's real exit reason, leaving no 'EXIT' message from it behind. A
%% child that was already gone gives noproc; while it is still linked, its
%% 'EXIT' message is on its way and carries the reason.
unlink_flush(Pid, noproc) ->
    {links, Links} = process_info(self(), links),
    Timeout = case lists:member(Pid, Links) of
                  true -> infinity;
                  false -> 0
              end,
    receive
        {'EXIT', Pid, Reason} -> Reason
    after Timeout ->
        child_process_unlinked
    end;
unlink_flush(Pid, Reason) ->
    unlink(Pid),
    receive
        {'EXIT', Pid, _} -> ok
    after 0 -> ok
    end,
    Reason.
