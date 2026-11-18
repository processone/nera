%% EXPECT: apply export visibility ok
%% MODULES: _apply_export_all.erl _apply_export_all_only.erl
-module(apply_export_visibility).
-export([test/0, callback/0, callback/3]).

%% External calls, even to this module, may only enter exported callbacks.
%% Run this unchanged on native Nera and BEAM.
test() ->
    lists:foreach(fun(Mode) ->
        {error, undef} = outcome(Mode, private_callback, []),
        {error, undef} = outcome(Mode, callback, [request]),
        {error, undef} = outcome(Mode, private_callback, [request, state, context]),
        undefined = erase(private_callback_entered),
        ready = invoke(Mode, callback, []),
        {request, state, context} = invoke(Mode, callback, [request, state, context]),
        {error, undef} = outcome(Mode, missing_callback, [])
    end, [apply, qualified_apply, module, function, both, static, tail_apply, tail_remote, remote_fun]),
    %% Local calls and local funs still have access to private functions.
    private = private_callback(),
    request = callback(request),
    {request, state, context} = private_callback(request, state, context),
    Local = fun callback/1,
    request = Local(request),
    request = apply(Local, [request]),
    {module, ?MODULE} = erlang:fun_info(Local, module),
    {name, callback} = erlang:fun_info(Local, name),
    {arity, 1} = erlang:fun_info(Local, arity),
    erase(private_callback_entered),
    %% export_all is a real export policy, including cross-module direct links.
    All = id(apply_export_all),
    ready = apply(All, callback, []),
    request = apply(All, callback, [request]),
    request = All:callback(request),
    ready = apply_export_all:callback(),
    request = All:tail(request),
    AllFun = fun All:callback/1,
    request = AllFun(request),
    %% Static calls, tail calls, remote funs and spawn/3 reach definitions that
    %% only export_all exports, with or without an -export list.
    request = apply_export_all:callback(request),
    request = apply_export_all:tail(request),
    request = static_tail(request),
    StaticFun = fun apply_export_all:callback/1,
    request = StaticFun(request),
    {ok, request} = apply_export_all_only:start_link(request),
    StartFun = fun apply_export_all_only:start_link/1,
    {ok, request} = StartFun(request),
    _ = spawn(apply_export_all_only, reply, [self(), request]),
    receive
        {apply_export_all_only, request} -> ok
    after 5000 -> error(spawn_timeout)
    end,
    io:format("apply export visibility ok~n"),
    ok.

static_tail(Value) -> apply_export_all:callback(Value).

outcome(Mode, Function, Args) ->
    try invoke(Mode, Function, Args) of
        Value -> {returned, Value}
    catch
        Class:Reason -> {Class, Reason}
    end.

invoke(apply, Function, Args) ->
    Result = apply(?MODULE, Function, Args),
    Result;
invoke(qualified_apply, Function, Args) ->
    Result = erlang:apply(?MODULE, Function, Args),
    Result;
invoke(module, Function, Args) ->
    Module = id(?MODULE),
    case {Function, Args} of
        {private_callback, []} -> Module:private_callback();
        {callback, []} -> Module:callback();
        {callback, [A]} -> Module:callback(A);
        {private_callback, [A, B, C]} -> Module:private_callback(A, B, C);
        {callback, [A, B, C]} -> Module:callback(A, B, C);
        {missing_callback, []} -> Module:missing_callback()
    end;
invoke(function, Function, Args) ->
    case Args of
        [] -> ?MODULE:Function();
        [A] -> ?MODULE:Function(A);
        [A, B, C] -> ?MODULE:Function(A, B, C)
    end;
invoke(both, Function, Args) ->
    Module = id(?MODULE),
    case Args of
        [] -> Module:Function();
        [A] -> Module:Function(A);
        [A, B, C] -> Module:Function(A, B, C)
    end;
invoke(static, Function, Args) ->
    case {Function, Args} of
        {private_callback, []} -> ?MODULE:private_callback();
        {callback, []} -> ?MODULE:callback();
        {callback, [A]} -> ?MODULE:callback(A);
        {private_callback, [A, B, C]} -> ?MODULE:private_callback(A, B, C);
        {callback, [A, B, C]} -> ?MODULE:callback(A, B, C);
        {missing_callback, []} -> ?MODULE:missing_callback()
    end;
invoke(tail_apply, Function, Args) ->
    apply(?MODULE, Function, Args);
invoke(tail_remote, Function, Args) ->
    tail_remote(id(?MODULE), Function, Args);
invoke(remote_fun, Function, Args) ->
    Module = id(?MODULE),
    Arity = length(Args),
    Fun = fun Module:Function/Arity,
    apply(Fun, Args).

tail_remote(Module, Function, []) -> Module:Function();
tail_remote(Module, Function, [A]) -> Module:Function(A);
tail_remote(Module, Function, [A, B, C]) -> Module:Function(A, B, C).

callback() -> ready.
callback(Value) -> put(private_callback_entered, true), Value.
callback(A, B, C) -> {A, B, C}.
private_callback() -> put(private_callback_entered, true), private.
private_callback(A, B, C) -> put(private_callback_entered, true), {A, B, C}.
id(Value) -> Value.
