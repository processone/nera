%% Adapted from OTP compiler test suites (Apache 2.0)
%% Tests tail call optimization with deep recursion.
-module(tail_recursion).
-export([test/0]).

test() ->
    %% Count down from a large number — would stack overflow without TCO
    ok = count_down(100000),

    %% Tail-recursive sum of a long list
    List = make_list(10000, []),
    50005000 = sum(List, 0),

    %% Tail-recursive state machine
    done = state_machine(start, 1000),

    %% Mutual recursion (not tail-optimized in general, but tests call depth)
    true = is_even(100),
    false = is_even(101),
    true = is_odd(101),
    false = is_odd(100),

    io:format("ok~n"),
    ok.

count_down(0) -> ok;
count_down(N) when N > 0 -> count_down(N - 1).

make_list(0, Acc) -> Acc;
make_list(N, Acc) -> make_list(N - 1, [N | Acc]).

sum([], Acc) -> Acc;
sum([H | T], Acc) -> sum(T, Acc + H).

state_machine(done, _) -> done;
state_machine(start, N) when N > 0 -> state_machine(running, N);
state_machine(running, N) when N > 1 -> state_machine(running, N - 1);
state_machine(running, 1) -> state_machine(done, 0).

is_even(0) -> true;
is_even(N) when N > 0 -> is_odd(N - 1).

is_odd(0) -> false;
is_odd(N) when N > 0 -> is_even(N - 1).
