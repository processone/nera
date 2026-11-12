-module(maps_stdlib).
-export([test/0]).

%% Helper to prevent constant folding
id(I) -> I.

test() ->
    test_new(),
    test_get_3(),
    test_keys(),
    test_values(),
    test_to_list(),
    test_from_list(),
    test_from_keys(),
    test_size(),
    test_merge(),
    test_update(),
    test_take(),
    test_map(),
    test_filter(),
    test_filtermap(),
    test_fold(),
    test_foreach(),
    test_with(),
    test_without(),
    test_update_with(),
    test_groups_from_list(),
    test_intersect(),
    ok.

%% --- BIF tests ---

test_new() ->
    M = maps:new(),
    0 = maps:size(M),
    [] = maps:to_list(M),
    ok.

test_get_3() ->
    M = id(#{a => 1, b => 2}),
    1 = maps:get(a, M, default),
    2 = maps:get(b, M, default),
    default = maps:get(c, M, default),
    ok.

test_keys() ->
    M = id(#{a => 1, b => 2, c => 3}),
    Keys = maps:keys(M),
    3 = length(Keys),
    true = lists:member(a, Keys),
    true = lists:member(b, Keys),
    true = lists:member(c, Keys),
    %% Empty map
    [] = maps:keys(id(#{})),
    ok.

test_values() ->
    M = id(#{a => 10, b => 20, c => 30}),
    Vals = maps:values(M),
    3 = length(Vals),
    true = lists:member(10, Vals),
    true = lists:member(20, Vals),
    true = lists:member(30, Vals),
    %% Empty map
    [] = maps:values(id(#{})),
    ok.

test_to_list() ->
    M = id(#{x => 1, y => 2}),
    L = maps:to_list(M),
    2 = length(L),
    true = lists:member({x, 1}, L),
    true = lists:member({y, 2}, L),
    %% Empty map
    [] = maps:to_list(id(#{})),
    ok.

test_from_list() ->
    M = maps:from_list(id([{a, 1}, {b, 2}, {c, 3}])),
    1 = maps:get(a, M),
    2 = maps:get(b, M),
    3 = maps:get(c, M),
    3 = maps:size(M),
    %% Later keys override earlier ones
    M2 = maps:from_list(id([{a, 1}, {b, 2}, {a, 99}])),
    99 = maps:get(a, M2),
    2 = maps:get(b, M2),
    2 = maps:size(M2),
    %% Empty list
    M3 = maps:from_list(id([])),
    0 = maps:size(M3),
    ok.

test_from_keys() ->
    M = maps:from_keys(id([a, b, c]), 0),
    0 = maps:get(a, M),
    0 = maps:get(b, M),
    0 = maps:get(c, M),
    3 = maps:size(M),
    %% Duplicate keys — deduplicated
    M2 = maps:from_keys(id([x, x, y]), 1),
    1 = maps:get(x, M2),
    1 = maps:get(y, M2),
    2 = maps:size(M2),
    %% Empty key list
    M3 = maps:from_keys(id([]), default),
    0 = maps:size(M3),
    ok.

test_size() ->
    0 = maps:size(id(#{})),
    1 = maps:size(id(#{a => 1})),
    3 = maps:size(id(#{a => 1, b => 2, c => 3})),
    ok.

test_merge() ->
    M1 = id(#{a => 1, b => 2}),
    M2 = id(#{b => 99, c => 3}),
    M3 = maps:merge(M1, M2),
    1 = maps:get(a, M3),
    99 = maps:get(b, M3),  %% M2 overrides
    3 = maps:get(c, M3),
    3 = maps:size(M3),
    %% Merge with empty
    M1 = maps:merge(M1, id(#{})),
    M1 = maps:merge(id(#{}), M1),
    ok.

test_update() ->
    M = id(#{a => 1, b => 2}),
    M2 = maps:update(a, 10, M),
    10 = maps:get(a, M2),
    2 = maps:get(b, M2),
    ok.

test_take() ->
    M = id(#{a => 1, b => 2, c => 3}),
    {1, M2} = maps:take(a, M),
    false = maps:is_key(a, M2),
    2 = maps:get(b, M2),
    3 = maps:get(c, M2),
    2 = maps:size(M2),
    %% Key not present
    error = maps:take(z, M),
    ok.

%% --- Erlang stdlib tests ---

test_map() ->
    M = id(#{a => 1, b => 2, c => 3}),
    M2 = maps:map(fun(_K, V) -> V * 10 end, M),
    10 = maps:get(a, M2),
    20 = maps:get(b, M2),
    30 = maps:get(c, M2),
    %% Empty map
    E = maps:map(fun(_K, V) -> V end, id(#{})),
    0 = maps:size(E),
    ok.

test_filter() ->
    M = id(#{a => 1, b => 2, c => 3, d => 4}),
    M2 = maps:filter(fun(_K, V) -> V > 2 end, M),
    false = maps:is_key(a, M2),
    false = maps:is_key(b, M2),
    3 = maps:get(c, M2),
    4 = maps:get(d, M2),
    2 = maps:size(M2),
    ok.

test_filtermap() ->
    M = id(#{a => 1, b => 2, c => 3}),
    M2 = maps:filtermap(fun(_K, V) ->
        case V of
            1 -> false;
            2 -> true;
            3 -> {true, 30}
        end
    end, M),
    false = maps:is_key(a, M2),
    2 = maps:get(b, M2),
    30 = maps:get(c, M2),
    2 = maps:size(M2),
    ok.

test_fold() ->
    M = id(#{a => 1, b => 2, c => 3}),
    Sum = maps:fold(fun(_K, V, Acc) -> V + Acc end, 0, M),
    6 = Sum,
    %% Collect keys
    Keys = maps:fold(fun(K, _V, Acc) -> [K | Acc] end, [], M),
    3 = length(Keys),
    %% Empty map
    0 = maps:fold(fun(_K, V, Acc) -> V + Acc end, 0, id(#{})),
    ok.

test_foreach() ->
    M = id(#{a => 1, b => 2}),
    ok = maps:foreach(fun(_K, _V) -> ok end, M),
    ok = maps:foreach(fun(_K, _V) -> ok end, id(#{})),
    ok.

test_with() ->
    M = id(#{a => 1, b => 2, c => 3, d => 4}),
    M2 = maps:with([a, c], M),
    1 = maps:get(a, M2),
    3 = maps:get(c, M2),
    2 = maps:size(M2),
    %% Key not in map — just ignored
    M3 = maps:with([a, z], M),
    1 = maps:size(M3),
    1 = maps:get(a, M3),
    %% Empty keys
    M4 = maps:with([], M),
    0 = maps:size(M4),
    ok.

test_without() ->
    M = id(#{a => 1, b => 2, c => 3, d => 4}),
    M2 = maps:without([a, c], M),
    false = maps:is_key(a, M2),
    2 = maps:get(b, M2),
    false = maps:is_key(c, M2),
    4 = maps:get(d, M2),
    2 = maps:size(M2),
    %% Remove nonexistent key — no error
    M3 = maps:without([z], M),
    4 = maps:size(M3),
    %% Empty keys
    M4 = maps:without([], M),
    4 = maps:size(M4),
    ok.

test_update_with() ->
    M = id(#{a => 1, b => 2}),
    %% update_with/3 — key exists
    M2 = maps:update_with(a, fun(V) -> V + 10 end, M),
    11 = maps:get(a, M2),
    2 = maps:get(b, M2),
    %% update_with/4 — key exists
    M3 = maps:update_with(b, fun(V) -> V * 2 end, 0, M),
    4 = maps:get(b, M3),
    %% update_with/4 — key missing, uses Init
    M4 = maps:update_with(c, fun(V) -> V + 1 end, 99, M),
    99 = maps:get(c, M4),
    ok.

test_groups_from_list() ->
    %% groups_from_list/2 — group by key function
    L = id([1, 2, 3, 4, 5, 6]),
    G = maps:groups_from_list(fun(X) ->
        case X rem 2 of
            0 -> even;
            1 -> odd
        end
    end, L),
    [1, 3, 5] = maps:get(odd, G),
    [2, 4, 6] = maps:get(even, G),
    %% groups_from_list/3 — with value transform
    G2 = maps:groups_from_list(
        fun(X) ->
            case X rem 2 of
                0 -> even;
                1 -> odd
            end
        end,
        fun(X) -> X * 10 end,
        L),
    [10, 30, 50] = maps:get(odd, G2),
    [20, 40, 60] = maps:get(even, G2),
    ok.

test_intersect() ->
    M1 = id(#{a => 1, b => 2, c => 3}),
    M2 = id(#{b => 20, c => 30, d => 40}),
    %% intersect/2 — values from M2 win
    M3 = maps:intersect(M1, M2),
    20 = maps:get(b, M3),
    30 = maps:get(c, M3),
    2 = maps:size(M3),
    false = maps:is_key(a, M3),
    false = maps:is_key(d, M3),
    %% intersect_with/3 — custom combiner
    M4 = maps:intersect_with(fun(_K, V1, V2) -> V1 + V2 end, M1, M2),
    22 = maps:get(b, M4),
    33 = maps:get(c, M4),
    2 = maps:size(M4),
    %% No common keys
    M5 = maps:intersect(id(#{a => 1}), id(#{b => 2})),
    0 = maps:size(M5),
    ok.
