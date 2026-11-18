%% EXPECT: stacktrace_depth_ok
%% Captured stacktraces keep BEAM's default backtrace_depth of 8 frames,
%% innermost first, wherever a trace is read: try and catch, raise/3,
%% {Reason, Stack} exit reasons and current_stacktrace. Run on Nera and BEAM.
-module(stacktrace_depth).
-export([test/0, crash/0]).

test() ->
    lists:foreach(fun check_caught/1, [error, throw, exit, badmatch]),
    ok = check_catch(),
    ok = check_raise(),
    ok = check_exit_reason(),
    ok = check_current_stacktrace(),
    io:format("stacktrace_depth_ok~n"),
    ok.

%% The twelve-call chain below, innermost first, cut at BEAM's default depth.
expected() -> [l12, l11, l10, l9, l8, l7, l6, l5].

check_caught(Mode) ->
    {_Class, _Reason, Stack} = captured(Mode),
    Expected = expected(),
    Expected = names(Stack),
    io:format("caught ~p: ~p frames~n", [Mode, length(Stack)]).

captured(Mode) ->
    try l1(Mode)
    catch Class:Reason:Stack -> {Class, Reason, Stack}
    end.

check_catch() ->
    {'EXIT', {boom, Stack}} = catch l1(error),
    Expected = expected(),
    Expected = names(Stack),
    io:format("catch: ~p frames~n", [length(Stack)]).

%% raise/3 keeps the first eight frames of a longer trace, as BEAM does.
check_raise() ->
    {error, boom, Stack} = captured(error),
    Longer = Stack ++ Stack,
    Raised = try erlang:raise(error, boom, Longer)
             catch error:boom:Caught -> Caught
             end,
    Stack = Raised,
    io:format("raise/3: ~p of ~p frames~n", [length(Raised), length(Longer)]).

%% An uncaught error's exit reason carries the same trace a catch sees.
check_exit_reason() ->
    {error, boom, Caught} = captured(error),
    {Pid, Ref} = spawn_monitor(fun() -> l1(error) end),
    receive
        {'DOWN', Ref, process, Pid, {boom, Stack}} ->
            Caught = Stack,
            io:format("exit reason: ~p frames~n", [length(Stack)])
    after 5000 -> error(missing_exit_reason)
    end.

check_current_stacktrace() ->
    {current_stacktrace, Stack} = innermost(l1(current)),
    Expected = expected(),
    Expected = names(Stack),
    io:format("current stacktrace: ~p frames~n", [length(Stack)]).

%% Every frame is named, so a frame past the eighth, or a runner frame, fails.
names(Stack) -> [F || {_, F, _, _} <- Stack].

innermost({Tag, Inner}) when Tag =/= current_stacktrace -> innermost(Inner);
innermost(Result) -> Result.

%% Keep each call non-tail: BEAM may omit tail callers from a stacktrace.
l1(Mode) -> {l1, l2(Mode)}.
l2(Mode) -> {l2, l3(Mode)}.
l3(Mode) -> {l3, l4(Mode)}.
l4(Mode) -> {l4, l5(Mode)}.
l5(Mode) -> {l5, l6(Mode)}.
l6(Mode) -> {l6, l7(Mode)}.
l7(Mode) -> {l7, l8(Mode)}.
l8(Mode) -> {l8, l9(Mode)}.
l9(Mode) -> {l9, l10(Mode)}.
l10(Mode) -> {l10, l11(Mode)}.
l11(Mode) -> {l11, l12(Mode)}.
l12(error) -> erlang:error(boom);
l12(throw) -> throw(boom);
l12(exit) -> exit(boom);
l12(badmatch) -> 1 = id(2), ok;
l12(current) -> erlang:process_info(self(), current_stacktrace).

%% Uncaught entry used by scripts/test-stacktrace-order.py to check stderr.
crash() -> {crash, l1(error)}.

id(X) -> X.
