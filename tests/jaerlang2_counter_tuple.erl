%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (counter.erl)
%% Tests: tuple construction, tuple pattern matching, arithmetic on tuple elements
-module(jaerlang2_counter_tuple).
-export([test/0]).

test() ->
    C0 = {counter, 0},
    C1 = bump(1, C0),
    {counter, 1} = C1,
    C2 = bump(5, C1),
    {counter, 6} = C2,
    6 = read(C2),
    0 = read(C0),
    ok.

bump(N, {counter, K}) -> {counter, N + K}.
read({counter, N}) -> N.
