-module(receive_bound_var).
-export([test/0]).

%% Tests for selective receive with bound variables in patterns (Bug #4).

test() ->
    %% 1. Basic: receive {Pid, Result} where Pid is bound
    Self = self(),
    P1 = spawn(fun() -> Self ! {self(), result1} end),
    P2 = spawn(fun() -> Self ! {self(), result2} end),
    timer:sleep(50),
    %% Receive specifically from P1
    R1 = receive {P1, V1} -> V1 after 1000 -> timeout end,
    result1 = R1,
    %% Receive specifically from P2
    R2 = receive {P2, V2} -> V2 after 1000 -> timeout end,
    result2 = R2,

    %% 2. Bound variable skips non-matching messages
    Tag = hello,
    self() ! {world, 1},
    self() ! {hello, 2},
    self() ! {hello, 3},
    R3 = receive {Tag, N} -> N after 1000 -> timeout end,
    2 = R3,

    %% 3. Case expression with outer-bound variable
    X = 42,
    R4 = case {42, ok} of
        {X, V3} -> V3
    end,
    ok = R4,

    %% 4. Match expression with outer-bound variable
    Y = foo,
    {Y, Val} = {foo, bar},
    bar = Val,

    io:format("ok~n"),
    ok.
