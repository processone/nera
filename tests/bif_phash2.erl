-module(bif_phash2).
-export([test/0]).

test() ->
    %% Deterministic: same term always returns same hash
    H1 = erlang:phash2(hello),
    H1 = erlang:phash2(hello),

    %% Result is a non-negative integer
    true = is_integer(H1),
    true = (H1 >= 0),

    %% 1-arg form: result < 2^27
    true = (H1 < 134217728),

    %% 2-arg form: result in range 0..Range-1
    H2 = erlang:phash2(42, 10),
    true = (H2 >= 0),
    true = (H2 < 10),

    %% Different terms usually produce different hashes
    Ha = erlang:phash2(foo),
    Hb = erlang:phash2(bar),
    true = (Ha =/= Hb),

    %% Works on various term types
    _ = erlang:phash2(42),
    _ = erlang:phash2([1, 2, 3]),
    _ = erlang:phash2({a, b, c}),
    _ = erlang:phash2(<<"binary">>),

    %% badarg on invalid range
    caught = try_badarg(fun() -> erlang:phash2(hello, id(0)) end),
    caught = try_badarg(fun() -> erlang:phash2(hello, id(-1)) end),

    ok.

try_badarg(F) ->
    try F()
    catch error:badarg -> caught
    end.

id(X) -> X.
