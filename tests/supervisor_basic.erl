%% Tests basic supervisor functionality.
%% MODULES: sup_test_worker.erl
-module(supervisor_basic).
-export([test/0]).
-export([init/1]).

test() ->
    %% Start supervisor with one child
    {ok, SupPid} = supervisor:start_link({local, test_sup}, ?MODULE, []),
    true = is_process_alive(SupPid),

    %% Worker should be running
    receive after 100 -> ok end,
    WorkerPid = whereis(test_worker),
    true = is_pid(WorkerPid),
    true = is_process_alive(WorkerPid),

    %% Crash the worker via cast (triggers {stop, crashed, State} in gen_server)
    gen_server:cast(WorkerPid, crash),
    receive after 500 -> ok end,

    %% Supervisor should have restarted the worker
    NewWorkerPid = whereis(test_worker),
    true = is_pid(NewWorkerPid),
    true = is_process_alive(NewWorkerPid),
    %% Should be a different pid after restart
    true = (NewWorkerPid =/= WorkerPid),

    io:format("ok~n"),
    ok.

%% Supervisor callback
init([]) ->
    {ok, {{one_for_one, 5, 10},
          [{test_worker, {sup_test_worker, start_link, []},
            permanent, 5000, worker, [sup_test_worker]}]}}.
