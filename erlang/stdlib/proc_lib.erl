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

%% start_link/3 — synchronous start (like gen_server start_link but lower-level).
%% Calls Mod:init(Args), waits for init_ack.
start_link(Mod, Fun, Args) ->
    Pid = spawn_link(Mod, Fun, Args),
    receive
        {init_ack, Pid, Return} -> Return
    end.

%% start_link/5 — start_link with name registration.
start_link(Mod, Fun, Args, _Timeout, _Opts) ->
    start_link(Mod, Fun, Args).
