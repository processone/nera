%% EXPECT: try_after_reraise_ok
%% A try ... after runs its after body on every way out of the body, the of
%% clauses and the catch clauses, then raises the same exception again with
%% its class, reason and stacktrace. Here the handler that catches it is in
%% the caller, or there is none and the process exits, so the try is the
%% only handler in its own function. An exception from the after body
%% replaces the held one. Run this same fixture on native Nera and BEAM.
-module(try_after_reraise).
-export([test/0]).

test() ->
    %% No catch clauses: each class reaches the caller, after one cleanup.
    {error, boom} = outcome(fun() -> try error(boom) after self() ! cleaned end end),
    [cleaned] = drain(),
    {throw, boom} = outcome(fun() -> try throw(boom) after self() ! cleaned end end),
    [cleaned] = drain(),
    {exit, boom} = outcome(fun() -> try exit(boom) after self() ! cleaned end end),
    [cleaned] = drain(),

    %% A failed call and a failed match in the body.
    {error, badarith} = outcome(fun() -> try id(1) + id(a) after self() ! cleaned end end),
    [cleaned] = drain(),
    {error, {badmatch, 2}} = outcome(fun() -> try 1 = id(2) after self() ! cleaned end end),
    [cleaned] = drain(),

    %% A normal return keeps the body's value.
    {ok, body} = outcome(fun() -> try id(body) after self() ! cleaned, ignored end end),
    [cleaned] = drain(),

    %% No catch clause matches: the original exception goes on.
    {error, boom} = outcome(fun() -> try error(boom) catch throw:_ -> caught after self() ! cleaned end end),
    [cleaned] = drain(),
    %% A catch clause body raises: its exception goes on.
    {error, again} = outcome(fun() -> try error(boom) catch error:boom -> error(again) after self() ! cleaned end end),
    [cleaned] = drain(),
    %% An of clause body raises.
    {error, in_of} = outcome(fun() -> try id(1) of 1 -> error(in_of) after self() ! cleaned end end),
    [cleaned] = drain(),

    %% The after body's own exception replaces the held one.
    {throw, second} = outcome(fun() -> try error(first) after throw(second) end end),
    {error, second} = outcome(fun() -> try error(first) after error(second) end end),
    {error, timeout_value} = outcome(fun() ->
        try error(first) after receive never -> no after id(-1) -> ok end end end),
    [] = drain(),

    %% The stacktrace passes through unchanged.
    true = same_stacktrace(),
    [] = drain(),

    %% The held exception survives an after body that waits for a message,
    %% times out in a receive, and collects garbage.
    Parent = self(),
    spawn(fun() -> receive after 20 -> Parent ! go end end),
    {error, {boom, [1, 2, 3]}} = outcome(fun() ->
        try error({boom, lists:seq(1, 3)})
        after
            receive go -> ok end,
            receive never -> no after 5 -> ok end,
            L = lists:seq(1, 100000),
            erlang:garbage_collect(),
            self() ! length(L)
        end end),
    [100000] = drain(),

    %% With no handler left, the process runs its after body and exits with
    %% the same reason.
    {Pid, Ref} = spawn_monitor(fun() -> try exit(bye) after Parent ! cleaned end end),
    receive {'DOWN', Ref, process, Pid, bye} -> ok
    after 2000 -> error(no_exit) end,
    [cleaned] = drain(),

    io:format("try_after_reraise_ok~n"),
    ok.

%% Calls F in a separate function, so F's try is the only handler in F.
outcome(F) ->
    try {ok, F()} catch Class:Reason -> {Class, Reason} end.

raise_and_keep() ->
    try error(boom)
    catch error:boom:Stack ->
        self() ! {stack, Stack},
        erlang:raise(error, boom, Stack)
    end.

same_stacktrace() ->
    try try raise_and_keep() after ok end
    catch error:boom:Stack -> receive {stack, Kept} -> Kept =:= Stack end
    end.

drain() -> receive M -> [M | drain()] after 0 -> [] end.

id(X) -> X.
