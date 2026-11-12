-module(process_flag_fullsweep).
-export([test/0]).

test() ->
    %% Set fullsweep_after to 0 (force major GC every time)
    Old = process_flag(fullsweep_after, 0),
    %% Old should be the default (65535)
    true = is_integer(Old),
    %% Allocate some garbage in a loop to trigger GC
    alloc_loop(100),
    %% Set it back to a higher value
    0 = process_flag(fullsweep_after, 10),
    alloc_loop(50),
    10 = process_flag(fullsweep_after, 65535),
    ok.

alloc_loop(0) -> ok;
alloc_loop(N) ->
    %% Allocate a list that becomes garbage on next iteration
    _L = lists:seq(1, 100),
    alloc_loop(N - 1).
