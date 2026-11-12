%% Minimal application module for nera.
%%
%% Supports:
%% - start/1, start/2  — start an application by name
%% - stop/1            — stop a running application
%% - get_env/2, get_env/3 — application environment (stub)
%%
%% Convention: application:start(foo) calls foo_app:start(normal, []).
%% Built-in apps (sasl, kernel, stdlib) are no-ops.
%%
-module(application).
-export([start/1, start/2,
         stop/1,
         get_env/2, get_env/3]).

%% start/1 — start an application.
%% Built-in OTP apps are silently skipped.
start(App) ->
    start(App, normal).

%% start/2 — start with type.
start(App, Type) ->
    case is_builtin_app(App) of
        true ->
            ok;
        false ->
            AppMod = app_module(App),
            case AppMod:start(Type, []) of
                {ok, _Pid} -> ok;
                {ok, _Pid, _State} -> ok;
                {error, Reason} -> {error, Reason};
                ok -> ok;
                %% ignore included, as when the top supervisor's init/1
                %% returns it: OTP reads any other value as a bad return.
                Other -> {error, {bad_return, {{AppMod, start, [Type, []]}, Other}}}
            end
    end.

%% stop/1 — stop a running application.
stop(App) ->
    case is_builtin_app(App) of
        true ->
            ok;
        false ->
            AppMod = app_module(App),
            AppMod:stop([]),
            ok
    end.

%% get_env/2 — get application environment variable (stub).
get_env(_App, _Key) ->
    undefined.

%% get_env/3 — get with default.
get_env(_App, _Key, Default) ->
    Default.

%% Internal: map application name to module name.
%% Convention: AppName -> AppName_app
app_module(App) ->
    AppStr = atom_to_list(App),
    list_to_atom(AppStr ++ "_app").

%% Internal: built-in apps that we skip.
is_builtin_app(sasl) -> true;
is_builtin_app(kernel) -> true;
is_builtin_app(stdlib) -> true;
is_builtin_app(crypto) -> true;
is_builtin_app(_) -> false.
