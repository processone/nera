-module(bench_copy).
-export([start/0]).
start() ->
    Pid = spawn(fun() -> receive stop -> ok end end),
    Pid ! {lists:seq(1, 30), 1152921504606846976, <<1,2,3,4>>},
    timer:sleep(20),
    T0 = erlang:monotonic_time(microsecond),
    snapshots(30000, Pid),
    T1 = erlang:monotonic_time(microsecond),
    copies(30000, Pid),
    T2 = erlang:monotonic_time(microsecond),
    Pid ! stop,
    io:format("scalar_snapshot ~p~nstatus_snapshot ~p~n", [T1-T0, T2-T1]),
    halt(0).
copies(0, _) -> ok;
copies(N, Pid) ->
    {status, waiting} = process_info(Pid, status),
    copies(N-1, Pid).
snapshots(0, _) -> ok;
snapshots(N, Pid) -> {message_queue_len, 1} = process_info(Pid, message_queue_len), snapshots(N-1, Pid).
