%% EXPECT: register_freed_before_signal_ok
%% A dead process's registered name is free before any linked or monitoring
%% process receives its 'EXIT' or 'DOWN', as on BEAM. A supervisor that
%% restarts a named child as soon as the 'EXIT' arrives must find the name
%% free.
%%
%% Each round registers the name again the moment the first signal from the
%% dead process arrives. The extra monitors keep the dead process's scheduler
%% busy delivering more 'DOWN' messages after that first signal, so a name
%% freed only after the signals would still be taken. Also run on BEAM.
-module(register_freed_before_signal).
-export([test/0]).
-export([init/1, handle_call/3, handle_cast/2]).

-define(NAME, freed_name).
-define(CHILD, freed_child).
-define(ROUNDS, 500).
-define(MONITORS, 32).
-define(RESTARTS, 100).

test() ->
    process_flag(trap_exit, true),
    Scenarios = [monitor_normal, monitor_crash, link_exit, link_killed,
                 link_cascade],
    Taken = [{Scenario, Count} || Scenario <- Scenarios,
                                  Count <- [rounds(Scenario, ?ROUNDS, 0)],
                                  Count =/= 0],
    [] = Taken,
    ok = supervisor_restarts(?RESTARTS),
    io:format("register_freed_before_signal_ok~n"),
    ok.

%% The number of rounds in which the name was still taken.
rounds(_Scenario, 0, Taken) ->
    Taken;
rounds(Scenario, N, Taken) ->
    case one_round(Scenario) of
        ok ->
            rounds(Scenario, N - 1, Taken);
        taken ->
            %% The next round registers the name again: wait for the late
            %% unregistration instead of failing that round too.
            ok = await_free(1000),
            rounds(Scenario, N - 1, Taken + 1)
    end.

one_round(Scenario) ->
    {Pid, Start, Signal, Reason} = setup(Scenario),
    Refs = [monitor(process, Pid) || _ <- lists:seq(1, ?MONITORS)],
    Start(),
    ok = await(Signal, Pid, Reason),
    Result = register_now(),
    Pending = case Signal of
                  down -> length(Refs) - 1;
                  exit -> length(Refs)
              end,
    ok = flush_downs(Pid, Pending),
    Result.

%% The named process exits normally.
setup(monitor_normal) ->
    Pid = named(spawn, fun() -> ok end),
    {Pid, fun() -> Pid ! go end, down, normal};
%% The named process exits with a reason of its own.
setup(monitor_crash) ->
    Pid = named(spawn, fun() -> exit(crash) end),
    {Pid, fun() -> Pid ! go end, down, crash};
%% A linked process that traps exits gets the 'EXIT', as a supervisor does.
setup(link_exit) ->
    Pid = named(spawn_link, fun() -> exit(crash) end),
    {Pid, fun() -> Pid ! go end, exit, crash};
%% exit(Pid, kill), as a supervisor's brutal_kill shutdown sends.
setup(link_killed) ->
    Pid = named(spawn_link, fun() -> ok end),
    {Pid, fun() -> exit(Pid, kill) end, exit, killed};
%% The named process dies because a process linked to it died.
setup(link_cascade) ->
    Parent = self(),
    Leader = spawn(fun() -> receive go -> exit(crash) end end),
    Pid = spawn(fun() ->
                        link(Leader),
                        true = register(?NAME, self()),
                        Parent ! {registered, self()},
                        receive never -> ok end
                end),
    receive {registered, Pid} -> ok end,
    {Pid, fun() -> Leader ! go end, down, crash}.

%% A process registered as ?NAME that runs Exit once it receives go.
named(Spawn, Exit) ->
    Parent = self(),
    Body = fun() ->
                   true = register(?NAME, self()),
                   Parent ! {registered, self()},
                   receive go -> Exit() end
           end,
    Pid = case Spawn of
              spawn -> spawn(Body);
              spawn_link -> spawn_link(Body)
          end,
    receive {registered, Pid} -> ok end,
    Pid.

await(down, Pid, Reason) ->
    receive {'DOWN', _, process, Pid, Reason} -> ok end;
await(exit, Pid, Reason) ->
    receive {'EXIT', Pid, Reason} -> ok end.

register_now() ->
    try register(?NAME, self()) of
        true ->
            true = unregister(?NAME),
            ok
    catch
        error:badarg -> taken
    end.

flush_downs(_Pid, 0) ->
    ok;
flush_downs(Pid, N) ->
    receive {'DOWN', _, process, Pid, _} -> flush_downs(Pid, N - 1) end.

await_free(0) ->
    {still_registered, whereis(?NAME)};
await_free(Tries) ->
    case whereis(?NAME) of
        undefined -> ok;
        _ -> receive after 1 -> await_free(Tries - 1) end
    end.

%% A supervisor restarts its killed named gen_server child each time. The
%% restart registers the child's name right after the supervisor gets the
%% 'EXIT'. The restart limit allows exactly ?RESTARTS restarts: a restart
%% that found the name taken and had to be tried again would exceed it and
%% stop the supervisor.
supervisor_restarts(N) ->
    {ok, Sup} = supervisor:start_link(?MODULE, sup),
    ok = kill_and_restart(whereis(?CHILD), N),
    Child = whereis(?CHILD),
    Ref = monitor(process, Child),
    exit(Sup, kill),
    receive {'EXIT', Sup, killed} -> ok end,
    receive {'DOWN', Ref, process, Child, killed} -> ok end,
    undefined = whereis(?CHILD),
    ok.

kill_and_restart({not_restarted, _} = Failure, _N) ->
    Failure;
kill_and_restart(_Child, 0) ->
    ok;
kill_and_restart(Child, N) ->
    exit(Child, kill),
    kill_and_restart(await_restart(Child, 1000), N - 1).

await_restart(Old, 0) ->
    {not_restarted, Old};
await_restart(Old, Tries) ->
    case whereis(?CHILD) of
        New when is_pid(New), New =/= Old -> New;
        _ -> receive after 1 -> await_restart(Old, Tries - 1) end
    end.

init(sup) ->
    {ok, {{one_for_one, ?RESTARTS, 3600},
          [{?CHILD, {gen_server, start_link, [{local, ?CHILD}, ?MODULE, child, []]},
            permanent, brutal_kill, worker, [?MODULE]}]}};
init(child) ->
    {ok, child}.

handle_call(_Request, _From, State) ->
    {reply, ok, State}.

handle_cast(_Request, State) ->
    {noreply, State}.
