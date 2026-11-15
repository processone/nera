%% EXPECT: catch_error_stacktrace_ok
%% Unary catch must return the real error stack, just like try/catch and BEAM.
-module(catch_error_stacktrace).
-export([test/0]).

test() ->
    lists:foreach(fun check_error/1, [error, badmatch, badarith]),
    ok = check_resumed_error(),
    ok = check_saved_stack(),
    %% The other catch results do not acquire an error stack wrapper.
    {outer, {middle, ok}} = caught(ok),
    thrown = caught(throw),
    {'EXIT', stopped} = caught(exit),
    %% An inner catch clears the exception; an outer catch leaves its value alone.
    {'EXIT', {boom, Nested}} = catch caught(error),
    ok = check_frames(Nested),
    %% A later error gets its own reason and stack, not the preceding catch's.
    {'EXIT', {{badmatch, 2}, Later}} = caught(badmatch),
    ok = check_frames(Later),
    io:format("catch_error_stacktrace_ok~n"),
    ok.

check_error(Mode) ->
    {error, Reason, TryStack} = captured(Mode),
    {'EXIT', {Reason, CatchStack}} = caught(Mode),
    %% The shared raising functions have identical frames, including metadata.
    %% BEAM can prepend a BIF frame (for div/2); compare the shared Erlang calls.
    %% Catching callers and runner frames can differ beyond that prefix.
    Prefix = lists:sublist(module_frames(TryStack), 3),
    Prefix = lists:sublist(module_frames(CatchStack), 3),
    Label = case Mode of
        {resumed_error, _} ->
            %% Native try/catch currently loses callers across suspension.
            %% Catch must preserve the same available frames, not fabricate them.
            [{?MODULE, inner, 1, Info} | _] = Prefix,
            true = is_list(Info),
            resumed_error;
        _ -> ok = check_frames(CatchStack), Mode
    end,
    io:format("~p: ~p~n", [Label, [{M, F, A} || {M, F, A, _} <- Prefix]]).

check_resumed_error() ->
    Parent = self(),
    {Pid, Ref} = spawn_monitor(fun() ->
        check_error({resumed_error, Parent}),
        Parent ! {resumed_done, self()}
    end),
    %% Both captured/1 and caught/1 must suspend before their error is released.
    release_waiter(Pid),
    release_waiter(Pid),
    receive {resumed_done, Pid} -> ok after 5000 -> error(missing_resumed_result) end,
    receive {'DOWN', Ref, process, Pid, normal} -> ok
    after 5000 -> error(missing_resumed_exit)
    end.

release_waiter(Pid) ->
    receive
        {stack_waiting, Pid} -> ok;
        {'DOWN', _, process, Pid, Reason} -> error({stack_waiter_failed, Reason})
    after 5000 -> error(missing_stack_waiter)
    end,
    await_suspension(Pid),
    Pid ! stack_go,
    ok.

%% Observe waiting rather than assume a timeout caused a real suspension.
await_suspension(Pid) ->
    case process_info(Pid, status) of
        {status, waiting} -> ok;
        undefined -> error(stack_waiter_exited);
        _ -> await_suspension(Pid)
    end.

check_frames(Stack) ->
    [{?MODULE, inner, 1, InnerInfo}, {?MODULE, middle, 1, MiddleInfo},
     {?MODULE, outer, 1, OuterInfo} | _] = module_frames(Stack),
    true = is_list(InnerInfo),
    true = is_list(MiddleInfo),
    true = is_list(OuterInfo),
    ok.

module_frames(Stack) ->
    [Frame || {?MODULE, _, _, _} = Frame <- Stack].

check_saved_stack() ->
    {error, boom, Original} = captured(error),
    %% Use real captured frames, not a fabricated trace. raise/3 must preserve
    %% every frame and its metadata, and an explicitly empty trace stays empty.
    lists:foreach(fun(Stack) ->
        {'EXIT', {reraised, Stack}} = catch erlang:raise(error, reraised, Stack),
        %% An exit or throw still returns its usual form, even with a saved stack.
        {'EXIT', stopped} = catch erlang:raise(exit, stopped, Stack),
        thrown = catch erlang:raise(throw, thrown, Stack)
    end, [Original, lists:sublist(Original, 3), []]),
    io:format("saved full, shortened and empty stacks preserved~n"),
    ok.

caught(Mode) -> catch outer(Mode).

captured(Mode) ->
    try outer(Mode)
    catch Class:Reason:Stack -> {Class, Reason, Stack}
    end.

%% Keep these calls non-tail so both native and BEAM retain the shared frames.
outer(Mode) -> {outer, middle(Mode)}.
middle(Mode) -> {middle, inner(Mode)}.
inner(error) -> erlang:error(boom);
inner(badmatch) -> 1 = id(2), ok;
inner(badarith) -> 1 div id(0);
inner({resumed_error, Parent}) ->
    Parent ! {stack_waiting, self()},
    receive stack_go -> erlang:error(boom) end;
inner(throw) -> throw(thrown);
inner(exit) -> exit(stopped);
inner(ok) -> ok.

id(X) -> X.
