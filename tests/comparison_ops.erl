%% Adapted from OTP compiler test suites (Apache 2.0)
%% Tests comparison and equality operators.
-module(comparison_ops).
-export([test/0]).

test() ->
    %% Equality
    true = id(1) == id(1),
    false = id(1) == id(2),
    true = id(1) /= id(2),
    false = id(1) /= id(1),

    %% Exact equality
    true = id(1) =:= id(1),
    false = id(1) =:= id(1.0),
    true = id(1) =/= id(1.0),
    false = id(1) =/= id(1),

    %% Ordering
    true = id(1) < id(2),
    false = id(2) < id(1),
    false = id(1) < id(1),
    true = id(2) > id(1),
    false = id(1) > id(2),
    false = id(1) > id(1),
    true = id(1) =< id(1),
    true = id(1) =< id(2),
    false = id(2) =< id(1),
    true = id(1) >= id(1),
    true = id(2) >= id(1),
    false = id(1) >= id(2),

    %% Atom comparison
    true = id(abc) < id(def),
    true = id(abc) == id(abc),
    false = id(abc) == id(def),

    %% Mixed type comparison (Erlang term ordering: number < atom < ...)
    true = id(1) < id(a),
    true = id(42) < id(foo),

    %% Tuple comparison
    true = id({1, 2}) < id({1, 3}),
    true = id({1, 2}) < id({2, 1}),
    true = id({1}) < id({1, 2}),
    true = id({1, 2}) == id({1, 2}),

    %% List comparison
    true = id([1, 2]) < id([1, 3]),
    true = id([1, 2]) == id([1, 2]),
    true = id([]) < id([1]),

    io:format("ok~n"),
    ok.

id(I) -> I.
