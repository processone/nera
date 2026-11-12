%% Tests that BIF functions raise error:badarg on invalid arguments
%% inside try-catch (not return the error atom as a normal value).
-module(bif_badarg).
-export([test/0]).

test() ->
    %% length on non-list
    caught = try_badarg(fun() -> length(id(not_a_list)) end),
    %% element on non-tuple
    caught = try_badarg(fun() -> element(1, id(not_a_tuple)) end),
    %% element out of range
    caught = try_badarg(fun() -> element(5, id({a, b})) end),
    %% abs on non-number
    caught = try_badarg(fun() -> abs(id(hello)) end),
    %% atom_to_list on non-atom
    caught = try_badarg(fun() -> atom_to_list(id(123)) end),
    %% list_to_atom on non-list
    caught = try_badarg(fun() -> list_to_atom(id(123)) end),
    %% integer_to_list on non-integer
    caught = try_badarg(fun() -> integer_to_list(id(hello)) end),
    %% list_to_integer on non-list
    caught = try_badarg(fun() -> list_to_integer(id(hello)) end),
    %% float_to_list on non-float
    caught = try_badarg(fun() -> float_to_list(id(hello)) end),
    %% list_to_tuple on non-list
    caught = try_badarg(fun() -> list_to_tuple(id(hello)) end),
    %% tuple_to_list on non-tuple
    caught = try_badarg(fun() -> tuple_to_list(id(hello)) end),
    %% setelement bad index
    caught = try_badarg(fun() -> setelement(0, id({a, b}), c) end),

    %% Normal cases still work
    3 = length(id([a, b, c])),
    b = element(2, id({a, b, c})),
    5 = abs(id(-5)),
    "hello" = atom_to_list(id(hello)),
    hello = list_to_atom(id("hello")),
    "42" = integer_to_list(id(42)),
    42 = list_to_integer(id("42")),
    {a, b, c} = list_to_tuple(id([a, b, c])),
    [a, b, c] = tuple_to_list(id({a, b, c})),
    {x, b, c} = setelement(1, id({a, b, c}), x),

    ok.

try_badarg(F) ->
    try F()
    catch error:badarg -> caught
    end.

id(X) -> X.
