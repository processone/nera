%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (lib_misc.erl every/3)
%% Tests: link/1, process_flag(trap_exit, true), EXIT signals, receive...after periodic timer
-module(jaerlang2_every).
-export([test/0]).

test() ->
    Self = self(),

    %% Start a worker process that we'll monitor with every/3
    Worker = spawn(fun() -> worker_loop(Self) end),

    %% Start every/3: calls Fun every 50ms, linked to Worker
    every(Worker, 50, fun() -> Self ! tick end),

    %% Collect a few ticks to confirm periodic execution
    ok = collect_ticks(3),

    %% Now kill the worker — every process should stop too (via EXIT link)
    Worker ! die,

    %% Verify no more ticks arrive after the worker dies
    sleep(200),
    no_more = flush_ticks(),

    ok.

collect_ticks(0) -> ok;
collect_ticks(N) ->
    receive
        tick -> collect_ticks(N - 1)
    after 2000 ->
        timeout
    end.

flush_ticks() ->
    receive
        tick -> flush_ticks()
    after 0 ->
        no_more
    end.

worker_loop(Parent) ->
    receive
        die ->
            ok;
        {ping, From} ->
            From ! pong,
            worker_loop(Parent)
    end.

%% every(Pid, Time, Fun) - linked to Pid, calls Fun() every Time ms.
%% When Pid exits, this process gets an EXIT signal and stops.
every(Pid, Time, Fun) ->
    spawn(fun() ->
              process_flag(trap_exit, true),
              link(Pid),
              every_loop(Pid, Time, Fun)
          end).

every_loop(Pid, Time, Fun) ->
    receive
        {'EXIT', Pid, _Why} ->
            true
    after Time ->
        Fun(),
        every_loop(Pid, Time, Fun)
    end.

sleep(T) ->
    receive
    after T -> true
    end.
