-module(map_update).
-export([test/0]).

%% Tests for map update syntax: M#{k => v} and M#{k := v}

test() ->
    M = #{a => 1, b => 2, c => 3},

    %% 1. Update existing key with :=
    M1 = M#{a := 10},
    10 = maps:get(a, M1),
    2 = maps:get(b, M1),

    %% 2. Update existing key with =>
    M2 = M#{b => 20},
    20 = maps:get(b, M2),
    1 = maps:get(a, M2),

    %% 3. Insert new key with =>
    M3 = M#{d => 4},
    4 = maps:get(d, M3),
    4 = map_size(M3),

    %% 4. Multiple updates in one expression
    M4 = M#{a := 100, b := 200, c := 300},
    100 = maps:get(a, M4),
    200 = maps:get(b, M4),
    300 = maps:get(c, M4),

    %% 5. Mixed => and := in one expression
    M5 = M#{a := 10, d => 4},
    10 = maps:get(a, M5),
    4 = maps:get(d, M5),

    %% 6. Original map is unchanged
    1 = maps:get(a, M),
    2 = maps:get(b, M),

    %% 7. Chained updates
    M6 = (M#{a := 10})#{b := 20},
    10 = maps:get(a, M6),
    20 = maps:get(b, M6),

    %% 8. Update with computed values
    M7 = M#{a := id(42)},
    42 = maps:get(a, M7),

    %% 9. Update variable base map from function
    M8 = update_map(M),
    99 = maps:get(a, M8),

    io:format("ok~n"),
    ok.

update_map(Map) ->
    Map#{a := 99}.

id(X) -> X.
