-module(bif_ceil_floor).
-export([test/0]).

test() ->
    %% ceil/1 — integers pass through
    42 = ceil(42),
    -7 = ceil(-7),
    0 = ceil(0),

    %% ceil/1 — floats
    4 = ceil(3.2),
    4 = ceil(3.7),
    -2 = ceil(-2.3),
    -2 = ceil(-2.9),
    5 = ceil(5.0),
    1 = ceil(0.1),
    0 = ceil(-0.1),
    true = is_integer(ceil(3.14)),

    %% floor/1 — integers pass through
    42 = floor(42),
    -7 = floor(-7),
    0 = floor(0),

    %% floor/1 — floats
    3 = floor(3.2),
    3 = floor(3.7),
    -3 = floor(-2.3),
    -3 = floor(-2.9),
    5 = floor(5.0),
    0 = floor(0.9),
    -1 = floor(-0.1),
    true = is_integer(floor(3.14)),

    %% ceil/floor — badarg on non-numbers
    caught = try_badarg(fun() -> ceil(id(hello)) end),
    caught = try_badarg(fun() -> floor(id(hello)) end),
    caught = try_badarg(fun() -> ceil(id([])) end),
    caught = try_badarg(fun() -> floor(id([])) end),

    ok.

try_badarg(F) ->
    try F()
    catch error:badarg -> caught
    end.

id(X) -> X.
