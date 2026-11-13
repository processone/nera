%% Minimal gen_server implementation for nera.
%%
%% Supports the subset of OTP gen_server needed by typical applications:
%% - start/3, start/4, start_link/3, start_link/4
%% - call/2, call/3
%% - cast/2
%% - reply/2
%% - handle_call, handle_cast, handle_info callbacks
%% - terminate callback
%% - a server that traps exits stops when its parent exits, as in OTP
%%
%% Not supported: sys protocol, code_change, format_status, hibernate.

-module(gen_server).
-export([start/3, start/4,
         start_link/3, start_link/4,
         call/2, call/3,
         cast/2,
         reply/2]).

%% start(Mod, Args, Opts) -> {ok, Pid} | ignore | {error, Reason}
start(Mod, Args, _Opts) ->
    start_internal(Mod, Args, false, none).

%% start({local, Name}, Mod, Args, Opts)
start({local, Name}, Mod, Args, _Opts) ->
    start_internal(Mod, Args, false, Name);
start(Mod, Args, Opts, _Extra) ->
    start(Mod, Args, Opts).

%% start_link(Mod, Args, Opts) -> {ok, Pid} | ignore | {error, Reason}
start_link(Mod, Args, _Opts) ->
    start_internal(Mod, Args, true, none).

%% start_link({local, Name}, Mod, Args, Opts)
start_link({local, Name}, Mod, Args, _Opts) ->
    start_internal(Mod, Args, true, Name).

%% A name that is already registered gives {error, {already_started, Pid}}.
%% Otherwise the caller monitors the new server from the moment it is spawned,
%% so it gets an answer even when init/1 crashes and no acknowledgement comes,
%% as OTP's proc_lib does: {error, Reason} for a caller that traps exits or did
%% not link. A linked caller that does not trap exits exits with Reason.
%% When init/1 returns {error, Reason} or ignore, the server answers with that
%% value and exits normally, so every caller gets it. As in OTP, the caller of
%% start_link is the server's parent, and a server started without a link is
%% its own parent.
start_internal(Mod, Args, Link, MaybeName) ->
    case whereis_name(MaybeName) of
        undefined -> spawn_server(Mod, Args, Link, MaybeName);
        Pid -> {error, {already_started, Pid}}
    end.

spawn_server(Mod, Args, Link, MaybeName) ->
    Parent = self(),
    Ref = make_ref(),
    SpawnOpts = case Link of
        true -> [link, monitor];
        false -> [monitor]
    end,
    {Pid, MRef} = spawn_opt(fun() -> init_it(Mod, Args, Parent, Link, Ref, MaybeName) end,
                            SpawnOpts),
    receive
        {ack, Ref, ok} ->
            demonitor_flush(MRef),
            {ok, Pid};
        {nack, Ref, Return} ->
            %% The server exits normally right after this answer.
            receive {'DOWN', MRef, process, Pid, _} -> ok end,
            flush_exit(Pid),
            Return;
        {'DOWN', MRef, process, Pid, Reason} ->
            flush_exit(Pid),
            {error, Reason}
    end.

init_it(Mod, Args, Parent, Link, Ref, MaybeName) ->
    case register_name(MaybeName) of
        true ->
            init_server(Mod, Args, Parent, Link, Ref, MaybeName);
        {false, Pid} ->
            Parent ! {nack, Ref, {error, {already_started, Pid}}}
    end.

init_server(Mod, Args, Parent, Link, Ref, MaybeName) ->
    case init_mod(Mod, Args) of
        {ok, {ok, State}} ->
            Parent ! {ack, Ref, ok},
            loop(loop_parent(Parent, Link), Mod, State);
        {ok, {ok, State, _Timeout}} ->
            Parent ! {ack, Ref, ok},
            loop(loop_parent(Parent, Link), Mod, State);
        {ok, {stop, Reason}} ->
            init_failed(Mod, MaybeName, Reason, not is_shutdown(Reason));
        {ok, {error, _} = Error} ->
            %% OTP 26 and later: a silent failure, without an error report.
            unregister_name(MaybeName),
            Parent ! {nack, Ref, Error};
        {ok, ignore} ->
            unregister_name(MaybeName),
            Parent ! {nack, Ref, ignore};
        {ok, Else} ->
            init_failed(Mod, MaybeName, {bad_return_value, Else}, true);
        {'EXIT', error, Reason, Stacktrace} ->
            init_failed(Mod, MaybeName, {Reason, Stacktrace}, true);
        {'EXIT', exit, Reason, _Stacktrace} ->
            init_failed(Mod, MaybeName, Reason, not is_shutdown(Reason))
    end.

%% A thrown value is init/1's return value, as in OTP.
init_mod(Mod, Args) ->
    try
        {ok, Mod:init(Args)}
    catch
        throw:Return -> {ok, Return};
        Class:Reason:Stacktrace -> {'EXIT', Class, Reason, Stacktrace}
    end.

%% Free the name before exiting, so the caller can start the server again as
%% soon as it gets {error, Reason}. Report says whether OTP logs the failure.
init_failed(Mod, MaybeName, Reason, Report) ->
    case Report of
        true -> error_logger:error_msg("~p:init/1 failed: ~p~n", [Mod, Reason]);
        false -> ok
    end,
    unregister_name(MaybeName),
    exit(Reason).

%% Reasons OTP does not report as a crash.
is_shutdown(normal) -> true;
is_shutdown(shutdown) -> true;
is_shutdown({shutdown, _}) -> true;
is_shutdown(_) -> false.

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

loop_parent(Parent, true) -> Parent;
loop_parent(_Starter, false) -> self().

demonitor_flush(MRef) ->
    demonitor(MRef),
    receive {'DOWN', MRef, process, _, _} -> ok after 0 -> ok end.

%% A caller that traps exits gets {'EXIT', Pid, Reason} through the link before
%% the 'DOWN' message; consume it, as OTP's proc_lib does.
flush_exit(Pid) ->
    unlink(Pid),
    receive {'EXIT', Pid, _} -> ok after 0 -> ok end.

loop(Parent, Mod, State) ->
    receive
        {'$gen_call', From, Request} ->
            case Mod:handle_call(Request, From, State) of
                {reply, Reply, NewState} ->
                    reply(From, Reply),
                    loop(Parent, Mod, NewState);
                {reply, Reply, NewState, _Timeout} ->
                    reply(From, Reply),
                    loop(Parent, Mod, NewState);
                {noreply, NewState} ->
                    loop(Parent, Mod, NewState);
                {noreply, NewState, _Timeout} ->
                    loop(Parent, Mod, NewState);
                {stop, Reason, Reply, NewState} ->
                    reply(From, Reply),
                    do_terminate(Mod, Reason, NewState);
                {stop, Reason, NewState} ->
                    do_terminate(Mod, Reason, NewState)
            end;
        {'$gen_cast', Msg} ->
            case Mod:handle_cast(Msg, State) of
                {noreply, NewState} ->
                    loop(Parent, Mod, NewState);
                {noreply, NewState, _Timeout} ->
                    loop(Parent, Mod, NewState);
                {stop, Reason, NewState} ->
                    do_terminate(Mod, Reason, NewState)
            end;
        {'EXIT', Parent, Reason} ->
            %% A server that traps exits stops with its parent's reason, as
            %% in OTP; one that does not is stopped by the link itself.
            do_terminate(Mod, Reason, State);
        Info ->
            case Mod:handle_info(Info, State) of
                {noreply, NewState} ->
                    loop(Parent, Mod, NewState);
                {noreply, NewState, _Timeout} ->
                    loop(Parent, Mod, NewState);
                {stop, Reason, NewState} ->
                    do_terminate(Mod, Reason, NewState)
            end
    end.

do_terminate(Mod, Reason, State) ->
    Mod:terminate(Reason, State),
    case Reason of
        normal -> exit(normal);
        shutdown -> exit(shutdown);
        {shutdown, _} -> exit(Reason);
        _ -> exit(Reason)
    end.

%% call/2 — synchronous call with default timeout (5000ms).
call(ServerRef, Request) ->
    call(ServerRef, Request, 5000).

%% call/3 — synchronous call with timeout.
call(ServerRef, Request, Timeout) ->
    Pid = resolve_name(ServerRef),
    Ref = monitor(process, Pid),
    Pid ! {'$gen_call', {self(), Ref}, Request},
    receive
        {Ref, Reply} ->
            demonitor(Ref),
            Reply;
        {'DOWN', Ref, process, _Pid, Reason} ->
            exit({Reason, {gen_server, call, [ServerRef, Request]}})
    after Timeout ->
        demonitor(Ref),
        exit(timeout)
    end.

%% cast/2 — asynchronous message.
cast(ServerRef, Msg) ->
    Pid = resolve_name(ServerRef),
    Pid ! {'$gen_cast', Msg},
    ok.

%% reply/2 — send a reply to a caller (used in handle_call or deferred).
reply({Pid, Ref}, Reply) ->
    Pid ! {Ref, Reply},
    ok.

%% Resolve a name reference to a PID.
resolve_name(Pid) when is_pid(Pid) -> Pid;
resolve_name(Name) when is_atom(Name) -> whereis(Name);
resolve_name({local, Name}) -> whereis(Name);
resolve_name({global, _Name}) -> error(global_not_supported).
