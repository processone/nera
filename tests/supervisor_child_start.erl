%% EXPECT: supervisor_child_start_ok
%% Regression: how supervisor:start_link/2 and a running one_for_one
%% supervisor handle a child's start result, as on BEAM (OTP 29).
%% - A child whose start returns {error, Reason}, a value that is not a start
%%   result, raises, exits or throws fails the supervisor's start with
%%   {error, {shutdown, {failed_to_start_child, Id, Reason}}}: later children
%%   are not started, children started earlier stop, a registered name is
%%   freed, no 'EXIT' or 'DOWN' message is left, and a linked caller that does
%%   not trap exits exits with that reason.
%% - A child whose start returns or throws ignore does not fail the start, and
%%   the supervisor starts the children after it.
%% - A failed restart is tried again, each try counted against the restart
%%   limit; past the limit the supervisor exits with shutdown. A restart that
%%   returns ignore keeps the supervisor running.
%% Before the fix, native Nera started without a failing child, failed with
%% the raw crash reason or case_clause, and left a child stopped after a failed
%% restart. Run this same fixture on native Nera and BEAM.
-module(supervisor_child_start).
-export([test/0, init/1, start/1]).

test() ->
    process_flag(trap_exit, true),
    failed_start(),
    ignored_child(),
    restart_retried(),
    restart_retries_exhausted(),
    restart_ignored(),
    io:format("supervisor_child_start_ok~n"),
    ok.

%% A child that fails to start fails the supervisor's start.
failed_start() ->
    Test = self(),
    {error, {shutdown, {failed_to_start_child, second, why}}} =
        supervisor:start_link(?MODULE, sup([worker(first, Test),
                                            child(second, {return, {error, why}}, Test),
                                            worker(third, Test)])),
    %% The child started before the failure stops; the one after it never
    %% starts.
    await_down(await_started(first)),
    no_messages(),

    %% How each failure reads, as OTP's supervisor reports it.
    {error, {shutdown, {failed_to_start_child, raised, {'EXIT', {{badmatch, 2}, Stack}}}}} =
        supervisor:start_link(?MODULE, sup([child(raised, raise, Test)])),
    true = is_list(Stack),
    no_messages(),
    {error, {shutdown, {failed_to_start_child, exited, {'EXIT', boom}}}} =
        supervisor:start_link(?MODULE, sup([child(exited, {exit, boom}, Test)])),
    no_messages(),
    %% A thrown value is the start function's return value.
    {error, {shutdown, {failed_to_start_child, thrown, x}}} =
        supervisor:start_link(?MODULE, sup([child(thrown, {throw, x}, Test)])),
    no_messages(),
    {error, {shutdown, {failed_to_start_child, thrown_error, why}}} =
        supervisor:start_link(?MODULE, sup([child(thrown_error, {throw, {error, why}}, Test)])),
    no_messages(),
    {error, {shutdown, {failed_to_start_child, bad, bad}}} =
        supervisor:start_link(?MODULE, sup([child(bad, {return, bad}, Test)])),
    no_messages(),
    {error, {shutdown, {failed_to_start_child, not_pid, {ok, not_a_pid}}}} =
        supervisor:start_link(?MODULE, sup([child(not_pid, {return, {ok, not_a_pid}}, Test)])),
    no_messages(),
    %% A gen_server child whose init/1 crashes.
    {error, {shutdown, {failed_to_start_child, init_crash, {{badmatch, 2}, _}}}} =
        supervisor:start_link(?MODULE, sup([child(init_crash, init_crash, Test)])),
    no_messages(),

    %% A registered name is freed.
    {error, {shutdown, {failed_to_start_child, second, why}}} =
        supervisor:start_link({local, supervisor_child_start_sup}, ?MODULE,
                              sup([child(second, {return, {error, why}}, Test)])),
    undefined = whereis(supervisor_child_start_sup),
    no_messages(),

    %% A linked caller that does not trap exits exits with the same reason.
    {exited, {shutdown, {failed_to_start_child, second, why}}} = in_plain_caller(
        fun() ->
            supervisor:start_link(?MODULE, sup([child(second, {return, {error, why}}, Test)]))
        end),
    ok.

%% A child whose start returns or throws ignore does not fail the start, and
%% the children after it start; nor does a child supervisor whose init/1
%% returns ignore.
ignored_child() ->
    Test = self(),
    {ok, Sup} = supervisor:start_link(?MODULE, sup([child(ignored, {return, ignore}, Test),
                                                    child(thrown_ignore, {throw, ignore}, Test),
                                                    child(nested, ignored_sup, Test),
                                                    worker(after_ignore, Test)])),
    _ = await_started(after_ignore),
    stop(Sup),
    ok.

%% A restart whose start fails is tried again until it succeeds within the
%% restart limit: here a returned error, then an exit, then a start.
restart_retried() ->
    Test = self(),
    Script = script([ok]),
    {ok, Sup} = supervisor:start_link(?MODULE, sup({one_for_one, 5, 10},
                                                   [child(flaky, {script, Script}, Test)])),
    First = await_started(flaky),
    set_script(Script, [{return, {error, not_now}}, {exit, not_now}, ok]),
    exit(First, kill),
    Second = await_started(flaky),
    true = Second =/= First,
    [] = script_left(Script),
    no_messages(),
    stop(Sup),
    ok.

%% Each try counts against the restart limit: with at most 2 restarts in
%% 10 seconds, the supervisor tries twice, then exits with shutdown.
restart_retries_exhausted() ->
    Test = self(),
    Script = script([ok]),
    {ok, Sup} = supervisor:start_link(?MODULE, sup({one_for_one, 2, 10},
                                                   [child(flaky, {script, Script}, Test)])),
    First = await_started(flaky),
    Failures = [{return, {error, not_now}} || _ <- lists:seq(1, 10)],
    set_script(Script, Failures),
    exit(First, kill),
    receive
        {'EXIT', Sup, shutdown} -> ok
    after 5000 ->
        error(supervisor_still_running)
    end,
    8 = length(script_left(Script)),
    no_messages(),
    ok.

%% A restart that returns ignore leaves the supervisor running: it still
%% restarts its other child afterwards.
restart_ignored() ->
    Test = self(),
    Script = script([ok]),
    {ok, Sup} = supervisor:start_link(?MODULE, sup({one_for_one, 5, 10},
                                                   [child(flaky, {script, Script}, Test),
                                                    worker(other, Test)])),
    Flaky = await_started(flaky),
    Other = await_started(other),
    set_script(Script, [{notify, {return, ignore}}]),
    exit(Flaky, kill),
    %% The supervisor is running the restart; it handles the other child's
    %% 'EXIT' only after it.
    receive {restarting, flaky} -> ok after 5000 -> error({not_restarted, flaky}) end,
    exit(Other, kill),
    _ = await_started(other),
    [] = script_left(Script),
    no_messages(),
    stop(Sup),
    ok.

%% Supervisor and worker callbacks.
init({sup, Flags, Children}) -> {ok, {Flags, Children}};
init({sup, ignore}) -> ignore;
init({worker, Id, Test}) ->
    Test ! {started, Id, self()},
    {ok, Id};
init(crash) -> 1 = id(2), {ok, crash}.

%% Child start functions.
start({Id, worker, Test}) ->
    gen_server:start_link(?MODULE, {worker, Id, Test}, []);
start({_Id, {return, Value}, _Test}) -> Value;
start({_Id, {throw, Value}, _Test}) -> throw(Value);
start({_Id, {exit, Reason}, _Test}) -> exit(Reason);
start({_Id, raise, _Test}) -> 1 = id(2), ignore;
start({_Id, init_crash, _Test}) -> gen_server:start_link(?MODULE, crash, []);
start({_Id, ignored_sup, _Test}) -> supervisor:start_link(?MODULE, {sup, ignore});
start({Id, {notify, Next}, Test}) ->
    Test ! {restarting, Id},
    start({Id, Next, Test});
start({Id, {script, Script}, Test}) ->
    [Next | Rest] = script_left(Script),
    set_script(Script, Rest),
    start({Id, Next, Test});
start({Id, ok, Test}) -> start({Id, worker, Test}).

sup(Children) -> sup({one_for_one, 1, 5}, Children).

sup(Flags, Children) -> {sup, Flags, Children}.

worker(Id, Test) -> child(Id, worker, Test).

child(Id, How, Test) ->
    {Id, {?MODULE, start, [{Id, How, Test}]}, permanent, 5000, worker, [?MODULE]}.

%% What the next starts of a scripted child do, in order.
script(Steps) ->
    Script = ets:new(supervisor_child_start_script, [public]),
    set_script(Script, Steps),
    Script.

set_script(Script, Steps) -> true = ets:insert(Script, {steps, Steps}).

script_left(Script) ->
    [{steps, Steps}] = ets:lookup(Script, steps),
    Steps.

await_started(Id) ->
    receive
        {started, Id, Pid} -> Pid
    after 5000 ->
        error({not_started, Id})
    end.

await_down(Pid) ->
    Ref = monitor(process, Pid),
    receive
        {'DOWN', Ref, process, Pid, _} -> ok
    after 5000 ->
        error({still_running, Pid})
    end.

stop(Sup) ->
    exit(Sup, kill),
    receive {'EXIT', Sup, killed} -> ok end,
    no_messages().

%% Run Start in a fresh process, which does not trap exits, and return
%% {returned, Result} when Start returns or {exited, Reason} when the process
%% exits first.
in_plain_caller(Start) ->
    Self = self(),
    {Pid, Ref} = spawn_monitor(fun() -> Self ! {returned, self(), Start()} end),
    receive
        {'DOWN', Ref, process, Pid, Reason} ->
            receive
                {returned, Pid, Result} -> {returned, Result}
            after 0 -> {exited, Reason}
            end
    after 5000 ->
        error(plain_caller_still_waiting)
    end.

no_messages() ->
    receive
        Msg -> error({unexpected_message, Msg})
    after 0 -> ok
    end.

id(X) -> X.
