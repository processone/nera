-module(bif_round_trunc_float).
-export([test/0]).

test() ->
    %% round/1 — integers pass through
    42 = round(42),
    -7 = round(-7),
    0 = round(0),

    %% round/1 — floats
    4 = round(3.7),
    -2 = round(-2.3),
    3 = round(2.5),
    -3 = round(-2.5),
    1 = round(0.5),
    0 = round(-0.4),
    1 = round(0.6),
    -1 = round(-0.6),
    true = is_integer(round(3.14)),

    %% round/1 — zero float
    0 = round(0.0),

    %% trunc/1 — integers pass through
    42 = trunc(42),
    -7 = trunc(-7),
    0 = trunc(0),

    %% trunc/1 — floats
    3 = trunc(3.7),
    -2 = trunc(-2.9),
    0 = trunc(0.9),
    0 = trunc(-0.9),
    0 = trunc(0.0),
    99 = trunc(99.999),
    -99 = trunc(-99.999),
    true = is_integer(trunc(3.14)),

    %% float/1 — integers to float
    true = is_float(float(42)),
    true = is_float(float(0)),
    true = is_float(float(-7)),
    true = is_float(float(1000000)),

    %% float/1 — floats pass through
    true = is_float(float(3.14)),
    true = is_float(float(-0.5)),
    true = is_float(float(0.0)),

    %% round/trunc/float — badarg on non-numbers
    caught = try_badarg(fun() -> round(id(hello)) end),
    caught = try_badarg(fun() -> trunc(id(hello)) end),
    caught = try_badarg(fun() -> float(id(hello)) end),
    caught = try_badarg(fun() -> round(id([])) end),
    caught = try_badarg(fun() -> trunc(id([])) end),
    caught = try_badarg(fun() -> float(id([])) end),

    ok.

try_badarg(F) ->
    try F()
    catch error:badarg -> caught
    end.

id(X) -> X.
