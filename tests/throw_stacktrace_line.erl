%% EXPECT: throw_stacktrace_line_ok
%% The top frame of a throw/1 stacktrace names the line of the throw call,
%% as error/1's frame does and as on BEAM, whether the throw is caught or
%% ends its process. Each thrower puts ?LINE in the value it throws, so the
%% line its frame must carry travels with the exception.
-module(throw_stacktrace_line).
-export([test/0, plain/1]).

test() ->
    ok = caught(throw, plain, fun plain/1),
    %% A call earlier in the body records its own, earlier line first.
    ok = caught(throw, after_call, fun after_call/1),
    %% The thrown value is built by a call on a later line; the frame names
    %% the line where the throw call starts.
    ok = caught(throw, spanning, fun spanning/1),
    ok = caught(throw, in_handler, fun in_handler/1),
    %% Controls: erlang:throw/1 and error/1 take the ordinary call path.
    ok = caught(throw, remote, fun remote/1),
    ok = caught(error, error_control, fun error_control/1),

    %% Caught by a try in the throwing function itself.
    {{same_function, SameLine}, SameStack} = same_function(id(same_function)),
    ok = top(same_function, 1, SameLine, SameStack),

    %% Funs and comprehension bodies run in frames named differently on BEAM
    %% and natively; only their line is compared.
    Fun = fun(X) -> throw({X, ?LINE}) end,
    try Fun(id(in_fun)) of
        FunValue -> error({no_throw, FunValue})
    catch throw:{in_fun, FunLine}:FunStack ->
        [{?MODULE, _, 1, FunInfo} | _] = FunStack,
        ok = line(FunLine, FunInfo)
    end,
    try [throw({X, ?LINE}) || X <- id([in_comprehension])] of
        CompValue -> error({no_throw, CompValue})
    catch throw:{in_comprehension, CompLine}:CompStack ->
        [{?MODULE, _, _, CompInfo} | _] = CompStack,
        ok = line(CompLine, CompInfo)
    end,

    %% Uncaught: the {{nocatch, Value}, Stacktrace} exit reason carries the
    %% same frame, for an MFA spawn and for a closure.
    {Pid, Ref} = spawn_monitor(?MODULE, plain, [uncaught_mfa]),
    {{nocatch, {uncaught_mfa, MfaLine}}, MfaStack} = down(Pid, Ref),
    ok = top(plain, 1, MfaLine, MfaStack),
    {Closure, ClosureRef} = spawn_monitor(fun() -> after_call(uncaught_closure) end),
    {{nocatch, {uncaught_closure, ClosureLine}}, ClosureStack} =
        down(Closure, ClosureRef),
    ok = top(after_call, 1, ClosureLine, ClosureStack),

    io:format("throw_stacktrace_line_ok~n"),
    ok.

plain(X) -> throw({X, ?LINE}).

after_call(X) ->
    Y = id(X),
    throw({Y, ?LINE}).

spanning(X) ->
    Line = ?LINE, throw(
        {id(X),
         Line}).

remote(X) -> erlang:throw({X, ?LINE}).

in_handler(X) ->
    try error(X)
    catch error:X -> throw({X, ?LINE})
    end.

same_function(X) ->
    try throw({X, ?LINE})
    catch throw:Thrown:Stack -> {Thrown, Stack}
    end.

error_control(X) -> error({X, ?LINE}).

caught(Class, Name, F) ->
    try F(id(Name)) of
        Value -> error({no_raise, Name, Value})
    catch
        Raised:{Name, Line}:Stack when Raised =:= Class ->
            top(Name, 1, Line, Stack)
    end.

top(Name, Arity, Line, [{?MODULE, Name, Arity, Info} | _]) ->
    line(Line, Info).

line(Line, Info) when is_integer(Line), Line > 0 ->
    {line, Line} = lists:keyfind(line, 1, Info),
    ok.

down(Pid, Ref) ->
    receive {'DOWN', Ref, process, Pid, Reason} -> Reason
    after 5000 -> error(missing_down)
    end.

id(X) -> X.
