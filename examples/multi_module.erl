-module(multi_module).
-export([start/0]).

%% Tests cross-module function calls via nera_rt_apply.
%% Calls functions in math_utils module.

start() ->
    D = math_utils:double(21),
    io:format("double(21) = ~w~n", [D]),

    S = math_utils:add(10, 32),
    io:format("add(10, 32) = ~w~n", [S]),

    Total = math_utils:sum_list([1, 2, 3, 4, 5]),
    io:format("sum_list([1,2,3,4,5]) = ~w~n", [Total]),

    io:format("Multi-module test passed~n").
