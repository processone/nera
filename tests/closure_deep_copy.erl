%% Regression test for closure-in-closure deep copy during message passing.
%% Closures captured inside another closure's env were shallow-copied,
%% leading to dangling pointers across process heaps.
-module(closure_deep_copy).
-export([test/0]).

test() ->
    Self = self(),

    %% Create a closure that captures another closure
    Inner = fun(X) -> X * 2 end,
    Outer = fun() -> Inner(21) end,

    %% Spawn a process that receives and executes the nested closure
    Pid = spawn(fun() ->
        receive
            {run, F} ->
                Result = F(),
                Self ! {result, Result}
        after 2000 ->
            Self ! {result, timeout}
        end
    end),

    %% Send the outer closure (which captures Inner) across processes
    Pid ! {run, Outer},

    %% The child must correctly deep-copy Inner when it receives Outer
    {result, 42} = receive M -> M after 2000 -> exit(no_result) end,

    %% Also test: closure captured in a list inside another closure's env
    F1 = fun(N) -> N + 10 end,
    F2 = fun() -> lists:map(F1, [1, 2, 3]) end,

    Pid2 = spawn(fun() ->
        receive
            {run2, G} ->
                R = G(),
                Self ! {result2, R}
        after 2000 ->
            Self ! {result2, timeout}
        end
    end),

    Pid2 ! {run2, F2},
    {result2, [11, 12, 13]} = receive M2 -> M2 after 2000 -> exit(no_result2) end,

    ok.
