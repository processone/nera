%% EXPECT: proc_lib_init_crash_ok
%% Intent: proc_lib:start_link/3,5 must not hang when the child dies before
%% init_ack. Trapping callers return {error, Reason}; non-trapping linked
%% callers exit with Reason, including BEAM's stack-bearing error/throw reasons.
%% Run this same real-process regression natively and on BEAM.
-module(proc_lib_init_crash).
-export([test/0, init/2]).

test() ->
    lists:foreach(fun(Arity) ->
        lists:foreach(fun(Mode) ->
            {returned, {error, Reason}} = in_caller(true, fun() ->
                %% Startup must preserve unrelated messages in the mailbox.
                self() ! unrelated,
                Result = start(Arity, Mode),
                receive unrelated -> ok after 0 -> error(lost_message) end,
                no_messages(),
                Result
            end),
            ok = check_reason(Mode, Reason),
            io:format("start_link/~p trap ~p: returned expected error~n",
                      [Arity, Mode]),
            {exited, ExitReason} = in_caller(false, fun() -> start(Arity, Mode) end),
            ok = check_reason(Mode, ExitReason),
            io:format("start_link/~p plain ~p: exited with expected reason~n",
                      [Arity, Mode])
        end, [error, exit, throw]),

        %% A normal child exit before acknowledgement must not hang either.
        lists:foreach(fun(Trap) ->
            {returned, {error, normal}} = in_caller(Trap, fun() ->
                Result = start(Arity, normal),
                no_messages(),
                Result
            end)
        end, [true, false]),
        io:format("start_link/~p normal: returned error without hanging~n", [Arity]),

        lists:foreach(fun(Ack) ->
            {returned, ok} = in_caller(true, fun() ->
                {ok, Pid} = start(Arity, {ack, Ack}),
                no_messages(),
                true = is_process_alive(Pid),
                Pid ! {stop, after_ack},
                receive {'EXIT', Pid, after_ack} -> ok
                after 5000 -> error(missing_link_after_ack)
                end,
                no_messages()
            end),
            %% An ack followed immediately by exit still returns the ack,
            %% retains the link's EXIT and leaves no internal DOWN behind.
            {returned, ok} = in_caller(true, fun() ->
                {ok, Pid} = start(Arity, {ack_exit, Ack}),
                receive {'EXIT', Pid, normal} -> ok
                after 5000 -> error(missing_exit_after_ack)
                end,
                no_messages()
            end),
            {exited, after_ack} = in_caller(false, fun() ->
                {ok, Pid} = start(Arity, {ack, Ack}),
                Pid ! {stop, after_ack},
                receive unexpected -> error(unexpected) end
            end),
            io:format("start_link/~p init_ack/~p: acknowledged, link preserved~n",
                      [Arity, Ack])
        end, [1, 2])
    end, [3, 5]),
    io:format("proc_lib_init_crash_ok~n"),
    ok.

start(3, Mode) -> proc_lib:start_link(?MODULE, init, [self(), Mode]);
start(5, Mode) -> proc_lib:start_link(?MODULE, init, [self(), Mode], infinity, []).

init(_Parent, error) -> 1 = id(2);
init(_Parent, exit) -> exit(boom);
init(_Parent, throw) -> throw(uncaught);
init(_Parent, normal) -> ok;
init(Parent, {ack_exit, Ack}) -> acknowledge(Parent, Ack);
init(Parent, {ack, Ack}) ->
    acknowledge(Parent, Ack),
    receive {stop, Reason} -> exit(Reason) end.

acknowledge(Parent, Ack) ->
    [Parent | _] = get('$ancestors'),
    {?MODULE, init, 2} = get('$initial_call'),
    case Ack of
        1 -> proc_lib:init_ack({ok, self()});
        2 -> proc_lib:init_ack(Parent, {ok, self()})
    end.

check_reason(error, {{badmatch, 2}, Stack}) -> check_stack(Stack);
check_reason(exit, boom) -> ok;
check_reason(throw, {{nocatch, uncaught}, Stack}) -> check_stack(Stack).

%% Check the callback frame, not engine-dependent callers or source locations.
check_stack([{?MODULE, init, 2, _} | _]) -> ok.

%% Wait for the caller's real exit before judging whether start_link returned.
%% The outer bound turns the original infinite wait into a test failure.
in_caller(Trap, Fun) ->
    Parent = self(),
    {Pid, Ref} = spawn_monitor(fun() ->
        process_flag(trap_exit, Trap),
        Result = Fun(),
        Parent ! {returned, self(), Result}
    end),
    receive
        {'DOWN', Ref, process, Pid, Reason} ->
            receive
                {returned, Pid, Result} ->
                    normal = Reason,
                    {returned, Result}
            after 0 -> {exited, Reason}
            end
    after 5000 ->
        exit(Pid, kill),
        error(start_link_still_waiting)
    end.

no_messages() ->
    receive Msg -> error({unexpected_message, Msg})
    after 0 -> ok
    end.

id(X) -> X.
