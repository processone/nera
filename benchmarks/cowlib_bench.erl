%% Throughput bench: runs cowlib_demo:checks() in a loop, no printing
%% inside the timed region. Same source on OTP and on Nera. Driven by
%% benchmarks/cowlib_bench.sh.
-module(cowlib_bench).
-export([start/0]).

now_ns() -> erlang:monotonic_time(nanosecond).

loop(_F, 0) -> ok;
loop(F, N) -> _ = F(), loop(F, N - 1).

%% Smallest power-of-two count whose loop takes at least Target ns.
calib(F, Target) -> calib(F, Target, 1).
calib(F, Target, N) ->
    T0 = now_ns(), loop(F, N), T = now_ns() - T0,
    if T >= Target -> N; true -> calib(F, Target, N * 2) end.

warm(F, Until) ->
    case now_ns() >= Until of
        true -> ok;
        false -> F(), warm(F, Until)
    end.

timed(F, N) -> T0 = now_ns(), loop(F, N), (now_ns() - T0) div N.

start() ->
    %% http2_machine.request is left out: it starts pending 5 s timers on
    %% every call, and native Nera keeps one OS thread per pending timer
    %% (the program aborts near 4096 threads, NERA-88).
    Checks = [C || {Name, _} = C <- cowlib_demo:checks(),
                   Name =/= "http2_machine.request"],
    Suite = fun() -> lists:foreach(fun({_, F}) -> F() end, Checks) end,
    warm(Suite, now_ns() + 1000000000),
    N = calib(Suite, 100000000),
    Rounds = [timed(Suite, N) || _ <- lists:seq(1, 11)],
    io:format("SUITE n=~w ns_per_pass=~w~n", [N, Rounds]),
    lists:foreach(
      fun({Name, F}) ->
              loop(F, 50),
              M = calib(F, 20000000),
              Ts = [timed(F, M) || _ <- lists:seq(1, 7)],
              io:format("CHECK ~s n=~w ns=~w~n", [Name, M, Ts])
      end, Checks),
    io:format("DONE~n").
