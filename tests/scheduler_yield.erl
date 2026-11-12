%% Exercises scheduler-yield behavior in tight self-tail-call loops.
-module(scheduler_yield).
-export([test/0]).

test() ->
    %% Non-tail caller must not observe nil when inner loop yields.
    ok = count_down(50000),

    %% Single-arg state must be preserved across repeated yields.
    15000 = accumulate(0, 15000),

    %% Multi-arg state must be preserved across repeated yields.
    {done, 10100, -9800} = multi_arg_loop(100, 200, 10000),

    io:format("ok~n"),
    ok.

count_down(0) -> ok;
count_down(N) when N > 0 -> count_down(N - 1).

accumulate(Acc, 0) -> Acc;
accumulate(Acc, N) when N > 0 -> accumulate(Acc + 1, N - 1).

multi_arg_loop(A, B, 0) -> {done, A, B};
multi_arg_loop(A, B, N) when N > 0 -> multi_arg_loop(A + 1, B - 1, N - 1).
