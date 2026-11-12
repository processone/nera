-module(bif_test).
-export([start/0]).

start() ->
    %% Test auto-imported type checks
    io:format("is_integer(42) = ~w~n", [is_integer(42)]),
    io:format("is_atom(hello) = ~w~n", [is_atom(hello)]),
    io:format("is_list([1,2]) = ~w~n", [is_list([1,2])]),
    io:format("is_tuple({a,b}) = ~w~n", [is_tuple({a,b})]),

    %% Test abs/max/min
    io:format("abs(-5) = ~w~n", [abs(-5)]),
    io:format("max(3, 7) = ~w~n", [max(3, 7)]),
    io:format("min(3, 7) = ~w~n", [min(3, 7)]),

    %% Test list operations
    A = [1, 2, 3],
    B = [4, 5],
    io:format("~w ++ ~w = ~w~n", [A, B, A ++ B]),

    %% Test bitwise shifts
    io:format("1 bsl 4 = ~w~n", [1 bsl 4]),
    io:format("16 bsr 2 = ~w~n", [16 bsr 2]),

    %% Test short-circuit boolean
    io:format("true andalso false = ~w~n", [true andalso false]),
    io:format("false orelse true = ~w~n", [false orelse true]),

    %% Test guards
    check_type(42),
    check_type(hello),
    check_type([1,2]).

check_type(X) when is_integer(X) ->
    io:format("~w is an integer~n", [X]);
check_type(X) when is_atom(X) ->
    io:format("~w is an atom~n", [X]);
check_type(X) when is_list(X) ->
    io:format("~w is a list~n", [X]).
