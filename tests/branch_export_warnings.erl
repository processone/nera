%% EXPECT: branch_export_warnings_ok
-module(branch_export_warnings).
-export([test/0]).

%% Branch bindings supply values read only after the branching expression.
test() ->
    lists:foreach(fun({Route, Code}) ->
        Code = case_body(Route),
        Code = case_pattern({Route, Code}),
        Code = if_body(Route),
        Code = nested(Route),
        Code = captured(Route),
        Code = scrutinee(Route),
        Code = try_input(Route),
        Code = receive_body(Route),
        Code = receive_pattern(Code)
    end, [{primary, 200}, {fallback, 503}]),
    504 = receive_timeout(),
    {ok, 204} = local_fun(primary),
    ok = unused(primary),
    ignored = shadowed(primary),
    io:format("branch_export_warnings_ok~n"),
    ok.

case_body(Route) ->
    case Route of
        primary -> Code = 200;
        fallback -> Code = 503
    end,
    Code.

case_pattern(Response) ->
    case Response of
        {primary, Code} -> ok;
        {fallback, Code} -> ok
    end,
    Code.

if_body(Route) ->
    if Route =:= primary -> Code = 200;
       true -> Code = 503
    end,
    Code.

nested(Route) ->
    Result = case Route of
        primary ->
            begin
                if Route =:= primary -> Code = 200;
                   true -> Code = 500
                end
            end;
        fallback -> Code = 503
    end,
    {Result, Code} = {Code, Result},
    Code.

captured(Route) ->
    case Route of
        primary -> Code = 200;
        fallback -> Code = 503
    end,
    Read = fun() -> Code end,
    Read().

scrutinee(Route) ->
    case (case Route of
        primary -> Code = 200;
        fallback -> Code = 503
    end) of
        _ -> Code
    end.

try_input(Route) ->
    try case Route of
        primary -> Code = 200;
        fallback -> Code = 503
    end of
        _ -> Code
    catch
        _:_ -> failed
    end.

receive_body(Route) ->
    self() ! Route,
    receive
        primary -> Code = 200;
        fallback -> Code = 503
    end,
    Code.

receive_pattern(Response) ->
    self() ! {response, Response},
    receive {response, Code} -> ok end,
    Code.

receive_timeout() ->
    receive
        response -> Code = 200
    after 0 -> Code = 504
    end,
    Code.

%% Reading an outer Code must not mark a fun-local Code as used.
local_fun(Route) ->
    Run = fun() ->
        case Route of
            primary -> LocalCode = 200;
            fallback -> LocalCode = 503
        end,
        ok
    end,
    LocalCode = 204,
    {Run(), LocalCode}.

%% Unread bindings keep their warnings in each branch and timeout body.
unused(Route) ->
    case Route of
        primary -> UnusedPrimary = 200;
        fallback -> UnusedFallback = 503
    end,
    if Route =:= primary -> UnusedIf = 200;
       true -> UnusedIf = 503
    end,
    receive response -> UnusedResponse = 200
    after 0 -> UnusedTimeout = 504
    end,
    ok.

%% A later fun head shadows Code instead of reading the branch bindings.
shadowed(Route) ->
    case Route of
        primary -> ShadowedCode = 200;
        fallback -> ShadowedCode = 503
    end,
    (fun(ShadowedCode) -> ShadowedCode end)(ignored).
