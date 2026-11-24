%% Minimal proc_lib implementation for nera.
%%
%% proc_lib wraps spawn/spawn_link to set up process metadata
%% ($ancestors, $initial_call in process dictionary) and integrates
%% with error_logger for crash reports.
%%
%% Supports: spawn_link/1, spawn_link/3, spawn/1, spawn/3,
%%           init_ack/1, init_ack/2, start_link/3, start_link/5

-module(proc_lib).
-export([spawn_link/1, spawn_link/3,
         spawn/1, spawn/3,
         init_ack/1, init_ack/2,
         start_link/3, start_link/5]).

%% spawn_link/1 — spawn_link with proc_lib metadata.
spawn_link(Fun) when is_function(Fun) ->
    Parent = self(),
    erlang:spawn_link(fun() ->
        put('$ancestors', [Parent]),
        put('$initial_call', {erlang, apply, [Fun, []]}),
        try Fun()
        catch
            Class:Reason ->
                error_logger:error_msg("Error in process ~p: ~p:~p~n",
                                       [self(), Class, Reason]),
                erlang:exit(Reason)
        end
    end).

%% spawn_link/3 — spawn_link with MFA.
spawn_link(Mod, Fun, Args) ->
    Parent = self(),
    erlang:spawn_link(fun() ->
        put('$ancestors', [Parent]),
        put('$initial_call', {Mod, Fun, length(Args)}),
        try apply(Mod, Fun, Args)
        catch
            Class:Reason ->
                error_logger:error_msg("Error in process ~p (~p:~p/~p): ~p:~p~n",
                                       [self(), Mod, Fun, length(Args), Class, Reason]),
                erlang:exit(Reason)
        end
    end).

%% spawn/1 — spawn (no link) with proc_lib metadata.
spawn(Fun) when is_function(Fun) ->
    Parent = self(),
    erlang:spawn(fun() ->
        put('$ancestors', [Parent]),
        put('$initial_call', {erlang, apply, [Fun, []]}),
        try Fun()
        catch
            Class:Reason ->
                error_logger:error_msg("Error in process ~p: ~p:~p~n",
                                       [self(), Class, Reason]),
                erlang:exit(Reason)
        end
    end).

%% spawn/3 — spawn (no link) with MFA.
spawn(Mod, Fun, Args) ->
    Parent = self(),
    erlang:spawn(fun() ->
        put('$ancestors', [Parent]),
        put('$initial_call', {Mod, Fun, length(Args)}),
        try apply(Mod, Fun, Args)
        catch
            Class:Reason ->
                error_logger:error_msg("Error in process ~p (~p:~p/~p): ~p:~p~n",
                                       [self(), Mod, Fun, length(Args), Class, Reason]),
                erlang:exit(Reason)
        end
    end).

%% init_ack/1 — shorthand for init_ack(Parent, Return).
%% Sends the ack to the parent (caller of start_link).
init_ack(Return) ->
    case get('$ancestors') of
        [Parent | _] -> Parent ! {init_ack, self(), Return};
        _ -> ok
    end,
    ok.

%% init_ack/2
init_ack(Parent, Return) ->
    Parent ! {init_ack, self(), Return},
    ok.

%% start_link/3 — apply Mod:Fun(Args), then wait for init_ack or child exit.
start_link(Mod, Fun, Args) ->
    start_link(Mod, Fun, Args, infinity, []).

%% start_link/5 — spawn options are not yet supported.
start_link(Mod, Fun, Args, Timeout, _Opts) ->
    Parent = self(),
    %% Monitor from creation: the child can die before the caller runs again.
    {Pid, MRef} = erlang:spawn_opt(fun() ->
        start_init(Parent, Mod, Fun, Args)
    end, [link, monitor]),
    receive
        {init_ack, Pid, Return} ->
            erlang:demonitor(MRef),
            receive {'DOWN', MRef, process, Pid, _} -> ok after 0 -> ok end,
            Return;
        {'DOWN', MRef, process, Pid, Reason} ->
            %% The link's EXIT precedes DOWN. Consume only this child's exit,
            %% as OTP does, leaving unrelated messages untouched.
            erlang:unlink(Pid),
            receive {'EXIT', Pid, _} -> ok after 0 -> ok end,
            {error, Reason}
    after Timeout ->
        %% Unlink before killing so a non-trapping caller survives the timeout.
        %% Await DOWN even for a trapping child: return only after it is dead.
        erlang:unlink(Pid),
        erlang:exit(Pid, kill),
        receive {'DOWN', MRef, process, Pid, _} -> ok end,
        receive {'EXIT', Pid, _} -> ok after 0 -> ok end,
        {error, timeout}
    end.

start_init(Parent, Mod, Fun, Args) ->
    put('$ancestors', [Parent]),
    put('$initial_call', {Mod, Fun, length(Args)}),
    try apply(Mod, Fun, Args)
    catch
        Class:Reason:Stacktrace ->
            error_logger:error_msg("Error in process ~p (~p:~p/~p): ~p:~p~n",
                                   [self(), Mod, Fun, length(Args), Class, Reason]),
            %% Preserve OTP's exit envelope, not just the caught reason.
            case Class of
                error -> erlang:exit({Reason, Stacktrace});
                throw -> erlang:exit({{nocatch, Reason}, Stacktrace});
                exit -> erlang:exit(Reason)
            end
    end.
