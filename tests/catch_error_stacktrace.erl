%% EXPECT: catch_error_stacktrace_ok
%% Unary catch must return the real error stack, just like try/catch and BEAM.
-module(catch_error_stacktrace).
-export([test/0]).

test() ->
    lists:foreach(fun check_error/1, [error, badmatch, badarith, resumed_error]),
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
    ok = check_frames(CatchStack),
    io:format("~p: ~p~n", [Mode, [{M, F, A} || {M, F, A, _} <- Prefix]]).

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
inner(resumed_error) -> receive after 0 -> erlang:error(boom) end;
inner(throw) -> throw(thrown);
inner(exit) -> exit(stopped);
inner(ok) -> ok.

id(X) -> X.
