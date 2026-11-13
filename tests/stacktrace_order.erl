%% EXPECT: stacktrace_order_ok
%% Stacktraces identify the raising function before its non-tail callers,
%% including re-raises and {Reason, Stack} exit reasons. Run on Nera and BEAM.
-module(stacktrace_order).
-export([test/0, init/1]).

test() ->
    {outer, {middle, ok}} = outer(ok),
    lists:foreach(fun check_caught/1, [error, throw, exit, badmatch]),
    lists:foreach(fun check_reraised/1, [error, throw, exit, badmatch]),
    ok = check_exit_reason(),
    ok = check_failed_start(),
    io:format("stacktrace_order_ok~n"),
    ok.

check_caught(Mode) ->
    {Class, Reason, Stack} = captured(Mode),
    {Class, Reason} = expected(Mode),
    ok = check_order(Stack),
    io:format("caught ~p: ~p~n", [Mode, first_calls(Stack)]).

captured(Mode) ->
    try outer(Mode)
    catch Class:Reason:Stack -> {Class, Reason, Stack}
    end.

%% Keep each call non-tail: BEAM may omit tail callers from a stacktrace.
outer(Mode) -> {outer, middle(Mode)}.
middle(Mode) -> {middle, inner(Mode)}.
inner(error) -> erlang:error(boom);
inner(throw) -> throw(boom);
inner(exit) -> exit(boom);
inner(badmatch) -> 1 = id(2), ok;
inner(ok) -> ok.

expected(error) -> {error, boom};
expected(throw) -> {throw, boom};
expected(exit) -> {exit, boom};
expected(badmatch) -> {error, {badmatch, 2}}.

check_order(Stack) ->
    [{?MODULE, inner, 1, _}, {?MODULE, middle, 1, _},
     {?MODULE, outer, 1, _} | _] = Stack,
    ok.

first_calls(Stack) ->
    [{M, F, A} || {M, F, A, _} <- lists:sublist(Stack, 3)].

check_reraised(Mode) ->
    {Class, Reason, Original} = captured(Mode),
    %% Also supply a shortened, real trace: raise/3 must keep list order,
    %% rather than capture the re-raising function's current call stack.
    lists:foreach(fun(Stack) ->
        Raised = try erlang:raise(Class, Reason, Stack)
                 catch Class:Reason:Caught -> Caught
                 end,
        Stack = Raised,
        ok = check_order(Raised)
    end, [Original, lists:sublist(Original, 3)]),
    io:format("reraised ~p: ~p~n", [Mode, first_calls(Original)]).

check_exit_reason() ->
    {Pid, Ref} = spawn_monitor(fun() ->
        {error, Reason, Stack} = captured(error),
        exit({Reason, Stack})
    end),
    receive
        {'DOWN', Ref, process, Pid, {boom, Stack}} ->
            ok = check_order(Stack),
            io:format("exit reason: ~p~n", [first_calls(Stack)])
    after 5000 -> error(missing_exit_reason)
    end.

check_failed_start() ->
    Previous = process_flag(trap_exit, true),
    {error, {boom, Stack}} = gen_server:start_link(?MODULE, [], []),
    process_flag(trap_exit, Previous),
    ok = check_order(Stack),
    io:format("failed start: ~p~n", [first_calls(Stack)]).

init([]) -> {ok, outer(error)}.
id(X) -> X.
