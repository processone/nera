-module(map_exact_update).
-export([test/0]).

id(X) -> X.

%% Tests that := (exact update) raises {badkey, Key} when key is missing,
%% while => (assoc) always succeeds.
%% Also tests that badmap/badkey errors use BEAM-compatible tuple format.

test() ->
    M = id(#{a => 1, b => 2}),

    %% 1. := on existing key succeeds
    M1 = M#{a := 10},
    10 = maps:get(a, M1),
    2 = maps:get(b, M1),

    %% 2. => on non-existing key succeeds (insert)
    M2 = M#{c => 3},
    3 = maps:get(c, M2),

    %% 3. := on non-existing key raises {badkey, Key}
    badkey_caught = try_exact_missing(),

    %% 4. Multiple := updates succeed when all keys exist
    M3 = M#{a := 100, b := 200},
    100 = maps:get(a, M3),
    200 = maps:get(b, M3),

    %% 5. Mixed => and := in one expression
    M4 = M#{a := 10, c => 3},
    10 = maps:get(a, M4),
    3 = maps:get(c, M4),

    %% 6. maps:get on missing key raises {badkey, Key}
    ok = try
        _ = maps:get(missing, id(M)),
        should_not_reach
    catch
        error:{badkey, missing} -> ok
    end,

    %% 7. map update on non-map raises {badmap, Term}
    ok = try
        _ = id(not_a_map)#{a := 1},
        should_not_reach
    catch
        error:{badmap, not_a_map} -> ok
    end,

    ok.

try_exact_missing() ->
    M = id(#{a => 1}),
    try
        M#{z := 99}
    catch
        error:{badkey, z} -> badkey_caught
    end.
