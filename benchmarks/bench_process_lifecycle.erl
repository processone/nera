%% Benchmark: process spawn/hibernate/wake lifecycle overhead.
%%
%% Measures:
%% 1. spawn_opt with link + monitor (N processes)
%% 2. hibernate + wake round-trip (N cycles per process)
%% 3. max_heap_size kill overhead (N processes)
%%
%% Usage: ./target/release/nera build benchmarks/bench_process_lifecycle.erl -o /tmp/b -e bench_process_lifecycle:start && /tmp/b
-module(bench_process_lifecycle).
-export([start/0, hibernate_worker/1]).

start() ->
    %% Phase 1: spawn_opt throughput — spawn N processes with link+monitor,
    %% wait for all to exit.
    N1 = 50000,
    T0 = erlang:monotonic_time(millisecond),
    spawn_opt_bench(N1),
    T1 = erlang:monotonic_time(millisecond),

    %% Phase 2: hibernate/wake round-trips — one process hibernates and
    %% wakes N times.
    N2 = 50000,
    T2 = erlang:monotonic_time(millisecond),
    hibernate_bench(N2),
    T3 = erlang:monotonic_time(millisecond),

    %% Phase 3: max_heap_size kill — spawn N processes that grow until
    %% killed, parent collects EXIT signals.
    N3 = 5000,
    T4 = erlang:monotonic_time(millisecond),
    max_heap_kill_bench(N3),
    T5 = erlang:monotonic_time(millisecond),

    io:format("bench_process_lifecycle:~n"),
    io:format("  spawn_opt(~w procs):    ~w ms~n", [N1, T1 - T0]),
    io:format("  hibernate(~w cycles):   ~w ms~n", [N2, T3 - T2]),
    io:format("  max_heap_kill(~w procs): ~w ms~n", [N3, T5 - T4]),
    Total = (T1 - T0) + (T3 - T2) + (T5 - T4),
    io:format("time_ms:~w~n", [Total]),
    halt(0).

%% --- Phase 1: spawn_opt throughput ---

spawn_opt_bench(N) ->
    process_flag(trap_exit, true),
    spawn_opt_loop(N),
    collect_exits(N).

spawn_opt_loop(0) -> ok;
spawn_opt_loop(N) ->
    spawn_opt(fun() -> ok end, [link]),
    spawn_opt_loop(N - 1).

collect_exits(0) -> ok;
collect_exits(N) ->
    receive
        {'EXIT', _, normal} -> collect_exits(N - 1)
    end.

%% --- Phase 2: hibernate/wake round-trips ---

hibernate_bench(N) ->
    Self = self(),
    Pid = spawn_link(fun() -> hibernate_loop_init(Self) end),
    receive ready -> ok end,
    hibernate_ping_loop(Pid, N),
    Pid ! stop,
    receive {'EXIT', Pid, normal} -> ok end.

hibernate_ping_loop(_Pid, 0) -> ok;
hibernate_ping_loop(Pid, N) ->
    Pid ! {ping, self()},
    receive pong -> ok end,
    hibernate_ping_loop(Pid, N - 1).

hibernate_loop_init(Parent) ->
    process_flag(trap_exit, true),
    Parent ! ready,
    erlang:hibernate(?MODULE, hibernate_worker, [none]).

hibernate_worker(_) ->
    receive
        {ping, From} ->
            From ! pong,
            erlang:hibernate(?MODULE, hibernate_worker, [none]);
        stop ->
            ok
    end.

%% --- Phase 3: max_heap_size kill ---

max_heap_kill_bench(N) ->
    process_flag(trap_exit, true),
    max_heap_spawn_loop(N),
    collect_killed(N).

max_heap_spawn_loop(0) -> ok;
max_heap_spawn_loop(N) ->
    spawn_opt(fun() -> grow([]) end, [link, {max_heap_size, 500}]),
    max_heap_spawn_loop(N - 1).

grow(Acc) ->
    grow([lists:seq(1, 20) | Acc]).

collect_killed(0) -> ok;
collect_killed(N) ->
    receive
        {'EXIT', _, killed} -> collect_killed(N - 1)
    end.
