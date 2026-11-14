%% EXPECT: ets_freed_before_signal_ok
%% A dead owner's tables must be gone before its first EXIT or DOWN, as on
%% BEAM. Recreate the named table immediately, including in a supervised
%% gen_server's init. Extra monitors expose cleanup delayed until after signals.
-module(ets_freed_before_signal).
-export([test/0, init/1, handle_call/3, handle_cast/2]).

-define(TABLE, freed_ets_table).
-define(CHILD, freed_ets_child).
-define(ROUNDS, 500).
-define(MONITORS, 32).
-define(RESTARTS, 100).

test() ->
    process_flag(trap_exit, true),
    lists:foreach(fun(Scenario) -> rounds(Scenario, ?ROUNDS) end,
                  [monitor_normal, monitor_crash, link_exit, link_killed,
                   link_cascade]),
    ok = supervisor_restarts(),
    io:format("ets_freed_before_signal_ok~n"),
    ok.

rounds(_Scenario, 0) -> ok;
rounds(Scenario, N) ->
    {Pid, Start, Signal, Reason} = setup(Scenario),
    Refs = [monitor(process, Pid) || _ <- lists:seq(1, ?MONITORS)],
    Start(),
    receive_signal(Signal, Pid, Reason),
    %% This must succeed on the first attempt, without waiting for cleanup.
    ?TABLE = ets:new(?TABLE, [named_table, public]),
    Self = self(),
    Self = ets:info(?TABLE, owner),
    [] = ets:lookup(?TABLE, old_owner),
    true = ets:insert(?TABLE, {replacement, N}),
    [{replacement, N}] = ets:lookup(?TABLE, replacement),
    true = ets:delete(?TABLE),
    Pending = case Signal of down -> length(Refs) - 1; exit -> length(Refs) end,
    flush_downs(Pid, Pending),
    rounds(Scenario, N - 1).

setup(monitor_normal) ->
    Pid = owner(spawn, fun() -> ok end),
    {Pid, fun() -> Pid ! go end, down, normal};
setup(monitor_crash) ->
    Pid = owner(spawn, fun() -> exit(crash) end),
    {Pid, fun() -> Pid ! go end, down, crash};
setup(link_exit) ->
    Pid = owner(spawn_link, fun() -> exit(crash) end),
    {Pid, fun() -> Pid ! go end, exit, crash};
setup(link_killed) ->
    Pid = owner(spawn_link, fun() -> ok end),
    {Pid, fun() -> exit(Pid, kill) end, exit, killed};
setup(link_cascade) ->
    Parent = self(),
    Leader = spawn(fun() -> receive go -> exit(crash) end end),
    Pid = spawn(fun() ->
                        link(Leader),
                        create_table(),
                        Parent ! {ready, self()},
                        receive never -> ok end
                end),
    receive {ready, Pid} -> ok end,
    {Pid, fun() -> Leader ! go end, down, crash}.

owner(Spawn, Exit) ->
    Parent = self(),
    Body = fun() ->
                   create_table(),
                   Parent ! {ready, self()},
                   receive go -> Exit() end
           end,
    Pid = case Spawn of spawn -> spawn(Body); spawn_link -> spawn_link(Body) end,
    receive {ready, Pid} -> ok end,
    Pid.

create_table() ->
    ?TABLE = ets:new(?TABLE, [named_table, public]),
    true = ets:insert(?TABLE, {old_owner, self()}),
    ok.

receive_signal(down, Pid, Reason) ->
    receive {'DOWN', _, process, Pid, Reason} -> ok end;
receive_signal(exit, Pid, Reason) ->
    receive {'EXIT', Pid, Reason} -> ok end.

flush_downs(_Pid, 0) -> ok;
flush_downs(Pid, N) ->
    receive {'DOWN', _, process, Pid, _} -> flush_downs(Pid, N - 1) end.

%% The restart limit permits exactly the requested restarts. A failed init
%% (badarg from ets:new) consumes another attempt and stops the supervisor.
supervisor_restarts() ->
    {ok, Sup} = supervisor:start_link(?MODULE, {sup, self()}),
    Child0 = receive {started, Initial} -> Initial end,
    ok = kill_and_restart(Child0, ?RESTARTS),
    Child = whereis(?CHILD),
    Ref = monitor(process, Child),
    exit(Sup, kill),
    receive {'EXIT', Sup, killed} -> ok end,
    receive {'DOWN', Ref, process, Child, killed} -> ok end,
    undefined = ets:info(?TABLE, owner),
    ok.

kill_and_restart(_Child, 0) -> ok;
kill_and_restart(Child, N) ->
    exit(Child, kill),
    New = receive
              {started, Next} when Next =/= Child -> Next;
              {'EXIT', Sup, Reason} -> exit({supervisor_stopped, Sup, Reason})
          end,
    New = ets:info(?TABLE, owner),
    ok = gen_server:call(New, ready),
    kill_and_restart(New, N - 1).

init({sup, Parent}) ->
    {ok, {{one_for_one, ?RESTARTS, 3600},
          [{?CHILD, {gen_server, start_link,
                     [{local, ?CHILD}, ?MODULE, {child, Parent}, []]},
            permanent, brutal_kill, worker, [?MODULE]}]}};
init({child, Parent}) ->
    create_table(),
    Parent ! {started, self()},
    {ok, child}.

handle_call(_Request, _From, State) -> {reply, ok, State}.
handle_cast(_Request, State) -> {noreply, State}.
