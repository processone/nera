-module(bif_size).
-export([test/0]).

test() ->
    %% size/1 on tuples
    0 = size({}),
    1 = size({a}),
    3 = size({1, 2, 3}),

    %% size/1 on binaries
    0 = size(<<>>),
    3 = size(<<1, 2, 3>>),
    5 = size(<<"hello">>),

    %% size/1 — badarg on atoms, lists, integers
    caught = try_badarg(fun() -> size(id(hello)) end),
    caught = try_badarg(fun() -> size(id([])) end),
    caught = try_badarg(fun() -> size(id(42)) end),

    ok.

try_badarg(F) ->
    try F()
    catch error:badarg -> caught
    end.

id(X) -> X.
