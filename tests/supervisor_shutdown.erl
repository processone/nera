%% EXPECT: supervisor_shutdown_ok
%% MODULES: _supervisor_shutdown_server.erl
%% Regression: how a one_for_one supervisor and a gen_server stop, as on
%% BEAM (OTP 29).
%% - A supervisor stops when its parent, the process that started it with
%%   start_link, exits or sends it an exit signal, and exits with the
%%   parent's reason, normal included. An exit signal from another process
%%   does not stop it.
%% - Before it exits, whether its parent stopped it, its restart limit was
%%   reached or a child failed to start, it stops its running children one
%%   by one, the last started first, by their shutdown setting: brutal_kill
%%   kills the child; a time in milliseconds sends shutdown and kills the
%%   child if it is still running after that time; infinity waits. A child
%%   supervisor stops its own children the same way, and a gen_server child
%%   that traps exits runs terminate/2, which it need not export. A child that
%%   is not running is skipped, and one that exited meanwhile is not waited
%%   for.
%% - An invalid child spec, such as a shutdown setting the supervisor could
%%   not stop the child by, fails the start with {start_spec, Error} before
%%   any child starts.
%% - A gen_server that traps exits stops with its parent's reason, after
%%   terminate/2, when its parent exits or sends it an exit signal; an exit
%%   signal from another process reaches handle_info/2, as does its
%%   starter's when it was started without a link.
%% Before the fix, native Nera's supervisor ignored its parent's exit, never
%% stopped its children itself, and left a child supervisor or a child that
%% traps exits running after it exited. Run this same fixture on native Nera
%% and BEAM.
-module(supervisor_shutdown).
-export([test/0, start/1]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2, terminate/2]).

test() ->
    process_flag(trap_exit, true),
    parent_shutdown(),
    child_already_gone(),
    parent_exits(normal),
    parent_exits(boom),
    other_process_exit(),
    restart_limit(),
    failed_start(),
    bad_child_spec(),
    gen_server_parent(),
    io:format("supervisor_shutdown_ok~n"),
    ok.

%% The parent's exit(Sup, shutdown) stops the children, the last started
%% first, then the supervisor.
parent_shutdown() ->
    Test = self(),
    {ok, Sup} = supervisor:start_link(
                  ?MODULE, sup(top, Test,
                               [worker(first, Test),
                                sup_child(inner, Test, [worker(inner_worker, Test)]),
                                plain_worker(plain, Test),
                                child(ignored, {return, ignore}, Test, 5000, worker),
                                no_terminate(no_terminate, Test),
                                stubborn(stubborn, Test, 300),
                                brutal(brutal, Test),
                                worker(last, Test)])),
    Sup = await_started(top),
    Children = await_all([first, inner, inner_worker, plain, no_terminate,
                          stubborn, brutal, last]),
    Watched = watch(Children),
    Started = erlang:monotonic_time(millisecond),
    exit(Sup, shutdown),
    receive
        {'EXIT', Sup, shutdown} -> ok
    after 5000 ->
        error(supervisor_still_running)
    end,
    %% The stubborn child, which traps exits and ignores them, was given its
    %% 300 milliseconds before it was killed.
    true = erlang:monotonic_time(millisecond) - Started >= 290,
    [] = running(Children),
    [{terminated, last, shutdown},
     {ignored_exit, stubborn, shutdown},
     {terminated, inner_worker, shutdown},
     {terminated, first, shutdown}] = stop_log(4),
    expect_downs(Watched, [{first, shutdown}, {inner, shutdown},
                           {inner_worker, shutdown}, {plain, shutdown},
                           {no_terminate, shutdown}, {stubborn, killed},
                           {brutal, killed}, {last, shutdown}]),
    no_messages().

%% A child that exits while the supervisor is stopping another one is not
%% waited for: here the first child is killed while the supervisor gives the
%% stubborn child its time.
child_already_gone() ->
    Test = self(),
    {ok, Sup} = supervisor:start_link(
                  ?MODULE, sup(top, Test, [worker(first, Test),
                                           stubborn(stubborn, Test, 300)])),
    Sup = await_started(top),
    Children = await_all([first, stubborn]),
    Watched = watch(Children),
    {first, First} = lists:keyfind(first, 1, Children),
    exit(Sup, shutdown),
    [{ignored_exit, stubborn, shutdown}] = stop_log(1),
    exit(First, kill),
    receive
        {'EXIT', Sup, shutdown} -> ok
    after 5000 ->
        error(supervisor_still_running)
    end,
    expect_downs(Watched, [{first, killed}, {stubborn, killed}]),
    no_messages().

%% When its parent exits, the supervisor stops its children, then exits with
%% the parent's reason.
parent_exits(Reason) ->
    Test = self(),
    Parent = spawn(fun() ->
        {ok, _} = supervisor:start_link(
                    ?MODULE, sup(top, Test,
                                 [worker(first, Test),
                                  sup_child(inner, Test, [worker(inner_worker, Test)])])),
        receive stop -> exit(Reason) end
    end),
    Sup = await_started(top),
    Children = await_all([first, inner, inner_worker]),
    Watched = watch([{top, Sup} | Children]),
    Parent ! stop,
    expect_downs(Watched, [{top, Reason}]),
    [] = running(Children),
    [{terminated, inner_worker, shutdown},
     {terminated, first, shutdown}] = stop_log(2),
    expect_downs(Watched, [{first, shutdown}, {inner, shutdown},
                           {inner_worker, shutdown}]),
    no_messages().

%% An exit signal from a process other than its parent does not stop the
%% supervisor.
other_process_exit() ->
    Test = self(),
    {ok, Sup} = supervisor:start_link(?MODULE, sup(top, Test, [worker(first, Test)])),
    Sup = await_started(top),
    First = await_started(first),
    {Other, Ref} = spawn_monitor(fun() -> exit(Sup, shutdown) end),
    receive {'DOWN', Ref, process, Other, normal} -> ok end,
    %% The supervisor handles its messages in order: once it has restarted
    %% the child, it has handled the other process's exit signal.
    exit(First, kill),
    Restarted = await_started(first),
    true = is_process_alive(Sup),
    exit(Sup, shutdown),
    receive {'EXIT', Sup, shutdown} -> ok after 5000 -> error(supervisor_still_running) end,
    false = is_process_alive(Restarted),
    [{terminated, first, shutdown}] = stop_log(1),
    no_messages().

%% Past its restart limit, the supervisor stops its other children, the last
%% started first, then exits with shutdown.
restart_limit() ->
    Test = self(),
    {ok, Sup} = supervisor:start_link(
                  ?MODULE, sup(top, Test, {one_for_one, 0, 10},
                               [worker(first, Test),
                                sup_child(inner, Test, [worker(inner_worker, Test)]),
                                worker(crashing, Test)])),
    Sup = await_started(top),
    Children = await_all([first, inner, inner_worker, crashing]),
    Watched = watch(Children),
    {crashing, Crashing} = lists:keyfind(crashing, 1, Children),
    exit(Crashing, kill),
    receive
        {'EXIT', Sup, shutdown} -> ok
    after 5000 ->
        error(supervisor_still_running)
    end,
    [] = running(Children),
    [{terminated, inner_worker, shutdown},
     {terminated, first, shutdown}] = stop_log(2),
    expect_downs(Watched, [{first, shutdown}, {inner, shutdown},
                           {inner_worker, shutdown}, {crashing, killed}]),
    no_messages().

%% When a child fails to start, the children started before it stop, the
%% last started first, before start_link returns.
failed_start() ->
    Test = self(),
    {error, {shutdown, {failed_to_start_child, bad, why}}} =
        supervisor:start_link(
          ?MODULE, sup(top, Test,
                       [worker(first, Test),
                        sup_child(inner, Test, [worker(inner_worker, Test)]),
                        stubborn(stubborn, Test, 200),
                        child(bad, {return, {error, why}}, Test, 5000, worker),
                        worker(never, Test)])),
    _ = await_started(top),
    Children = await_all([first, inner, inner_worker, stubborn]),
    [] = running(Children),
    [{ignored_exit, stubborn, shutdown},
     {terminated, inner_worker, shutdown},
     {terminated, first, shutdown}] = stop_log(3),
    no_messages().

%% An invalid child spec fails the start before any child starts.
bad_child_spec() ->
    Test = self(),
    {error, {start_spec, {invalid_shutdown, forever}}} =
        supervisor:start_link(?MODULE, sup(top, Test,
                                           [worker(first, Test),
                                            child(bad, {worker, true}, Test, forever, worker)])),
    {error, {start_spec, {invalid_mfa, {?MODULE, start}}}} =
        supervisor:start_link(?MODULE, sup(top, Test,
                                           [worker(first, Test),
                                            {bad, {?MODULE, start}, permanent, 5000, worker,
                                             [?MODULE]}])),
    {error, {start_spec, {invalid_child_spec, not_a_spec}}} =
        supervisor:start_link(?MODULE, sup(top, Test, [worker(first, Test), not_a_spec])),
    {error, {start_spec, {duplicate_child_name, first}}} =
        supervisor:start_link(?MODULE, sup(top, Test, [worker(first, Test),
                                                       worker(first, Test)])),
    _ = [await_started(top) || _ <- [1, 2, 3, 4]],
    no_messages().

%% A gen_server that traps exits stops when its parent sends it an exit
%% signal or exits; another process's exit signal reaches handle_info/2.
gen_server_parent() ->
    Test = self(),
    {ok, Server} = gen_server:start_link(?MODULE, {worker, server, Test, true}, []),
    Server = await_started(server),
    {Other, Ref} = spawn_monitor(fun() -> exit(Server, shutdown) end),
    receive {'DOWN', Ref, process, Other, normal} -> ok end,
    receive
        {info, server, {'EXIT', Other, shutdown}} -> ok
    after 5000 ->
        error(no_handle_info)
    end,
    exit(Server, shutdown),
    receive {'EXIT', Server, shutdown} -> ok after 5000 -> error(server_still_running) end,
    [{terminated, server, shutdown}] = stop_log(1),

    Parent = spawn(fun() ->
        {ok, _} = gen_server:start_link(?MODULE, {worker, server, Test, true}, []),
        receive stop -> ok end
    end),
    Watched = watch([{server, await_started(server)}]),
    Parent ! stop,
    expect_downs(Watched, [{server, normal}]),
    [{terminated, server, normal}] = stop_log(1),

    %% A server started without a link is its own parent, so its starter's
    %% exit signal reaches handle_info/2.
    Starter = spawn(fun() ->
        {ok, Unlinked} = gen_server:start(?MODULE, {worker, unlinked, Test, true}, []),
        link(Unlinked),
        Test ! {linked, Unlinked},
        receive stop -> exit(bye) end
    end),
    Unlinked = await_started(unlinked),
    receive {linked, Unlinked} -> ok after 5000 -> error(not_linked) end,
    Starter ! stop,
    receive
        {info, unlinked, {'EXIT', Starter, bye}} -> ok
    after 5000 ->
        error(no_handle_info)
    end,
    true = is_process_alive(Unlinked),
    UnlinkedWatched = watch([{unlinked, Unlinked}]),
    exit(Unlinked, kill),
    expect_downs(UnlinkedWatched, [{unlinked, killed}]),
    no_messages().

%% Supervisor and gen_server callbacks.
init({sup, Id, Test, Flags, Children}) ->
    Test ! {started, Id, self()},
    {ok, {Flags, Children}};
init({worker, Id, Test, TrapExit}) ->
    process_flag(trap_exit, TrapExit),
    Test ! {started, Id, self()},
    {ok, {Id, Test}}.

handle_call(_Request, _From, State) -> {reply, ok, State}.

handle_cast(_Msg, State) -> {noreply, State}.

handle_info(Info, {Id, Test} = State) ->
    Test ! {info, Id, Info},
    {noreply, State}.

terminate(Reason, {Id, Test}) ->
    Test ! {terminated, Id, Reason},
    ok.

%% Child start functions.
start({Id, {worker, TrapExit}, Test}) ->
    gen_server:start_link(?MODULE, {worker, Id, Test, TrapExit}, []);
start({Id, {sup, Children}, Test}) ->
    supervisor:start_link(?MODULE, sup(Id, Test, Children));
start({Id, stubborn, Test}) ->
    %% Return once the child traps exits, so the supervisor cannot stop it
    %% before then.
    Starter = self(),
    Pid = spawn_link(fun() ->
                         process_flag(trap_exit, true),
                         Test ! {started, Id, self()},
                         Starter ! {trapping, self()},
                         ignore_exits(Id, Test)
                     end),
    receive {trapping, Pid} -> {ok, Pid} end;
start({_Id, {return, Value}, _Test}) ->
    Value.

%% A child that traps exits and ignores them, so only a kill stops it.
ignore_exits(Id, Test) ->
    receive
        {'EXIT', _From, Reason} ->
            Test ! {ignored_exit, Id, Reason},
            ignore_exits(Id, Test)
    end.

sup(Id, Test, Children) -> sup(Id, Test, {one_for_one, 1, 5}, Children).

sup(Id, Test, Flags, Children) -> {sup, Id, Test, Flags, Children}.

%% A gen_server that traps exits, so it runs terminate/2 when it is stopped.
worker(Id, Test) -> child(Id, {worker, true}, Test, 5000, worker).

%% A gen_server that does not trap exits.
plain_worker(Id, Test) -> child(Id, {worker, false}, Test, 5000, worker).

brutal(Id, Test) -> child(Id, {worker, true}, Test, brutal_kill, worker).

%% A gen_server that traps exits and does not export terminate/2.
no_terminate(Id, Test) ->
    {Id, {supervisor_shutdown_server, start_link, [Id, Test]}, permanent, 5000,
     worker, [supervisor_shutdown_server]}.

stubborn(Id, Test, Time) -> child(Id, stubborn, Test, Time, worker).

sup_child(Id, Test, Children) ->
    child(Id, {sup, Children}, Test, infinity, supervisor).

child(Id, How, Test, Shutdown, Type) ->
    {Id, {?MODULE, start, [{Id, How, Test}]}, permanent, Shutdown, Type, [?MODULE]}.

await_started(Id) ->
    receive
        {started, Id, Pid} -> Pid
    after 5000 ->
        error({not_started, Id})
    end.

await_all(Ids) -> [{Id, await_started(Id)} || Id <- Ids].

running(Children) -> [Id || {Id, Pid} <- Children, is_process_alive(Pid)].

%% The terminate/2 calls and ignored exit signals, in the order they came.
stop_log(0) -> [];
stop_log(N) ->
    receive
        {terminated, _, _} = Event -> [Event | stop_log(N - 1)];
        {ignored_exit, _, _} = Event -> [Event | stop_log(N - 1)]
    after 5000 ->
        error({stop_log_missing, N})
    end.

watch(Children) -> [{Id, Pid, monitor(process, Pid)} || {Id, Pid} <- Children].

expect_downs(Watched, Expected) ->
    lists:foreach(
      fun({Id, Reason}) ->
          {Id, Pid, Ref} = lists:keyfind(Id, 1, Watched),
          receive
              {'DOWN', Ref, process, Pid, Reason} -> ok;
              {'DOWN', Ref, process, Pid, Other} -> error({Id, Other})
          after 5000 ->
              error({still_running, Id})
          end
      end, Expected).

no_messages() ->
    receive
        Msg -> error({unexpected_message, Msg})
    after 0 -> ok
    end.
