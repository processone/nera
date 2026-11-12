%% Regression test for map operations with binary keys.
%%
%% Binary keys in maps must use semantic equality (byte comparison),
%% not pointer equality. Two separately-allocated binaries with the
%% same content must match as the same key.
-module(map_binary_keys).
-export([test/0]).

test() ->
    %% ── maps:get with compile-time binary key ──
    M1 = #{<<"key">> => <<"val">>},
    <<"val">> = maps:get(<<"key">>, M1),

    %% ── maps:get with runtime-created binary key ──
    K = list_to_binary("key"),
    M2 = #{K => <<"val">>},
    <<"val">> = maps:get(<<"key">>, M2),  % compile-time lookup key
    <<"val">> = maps:get(K, M2),          % runtime lookup key

    %% ── Cross-allocation lookup: build map one way, lookup another ──
    M3 = maps:put(list_to_binary("hello"), 42, #{}),
    42 = maps:get(<<"hello">>, M3),

    %% ── maps:is_key ──
    true = maps:is_key(<<"key">>, M1),
    false = maps:is_key(<<"nope">>, M1),
    true = maps:is_key(<<"hello">>, M3),

    %% ── maps:find ──
    {ok, <<"val">>} = maps:find(<<"key">>, M1),
    error = maps:find(<<"nope">>, M1),

    %% ── maps:remove ──
    M4 = maps:remove(<<"key">>, M1),
    0 = map_size(M4),
    %% Remove with runtime key
    M5 = maps:remove(list_to_binary("hello"), M3),
    0 = map_size(M5),

    %% ── maps:put update (key already exists) ──
    M6 = maps:put(<<"key">>, <<"new">>, M1),
    <<"new">> = maps:get(<<"key">>, M6),
    1 = map_size(M6),  % should update, not add duplicate

    %% ── Map update operator (:=) ──
    M7 = M1#{<<"key">> := <<"updated">>},
    <<"updated">> = maps:get(<<"key">>, M7),

    %% ── maps:merge with binary keys ──
    MA = #{<<"a">> => 1, <<"b">> => 2},
    MB = #{<<"b">> => 99, <<"c">> => 3},
    Merged = maps:merge(MA, MB),
    1 = maps:get(<<"a">>, Merged),
    99 = maps:get(<<"b">>, Merged),  % MB overrides
    3 = maps:get(<<"c">>, Merged),

    %% ── maps:from_list with binary keys ──
    M8 = maps:from_list([{<<"x">>, 10}, {<<"y">>, 20}]),
    10 = maps:get(<<"x">>, M8),
    20 = maps:get(<<"y">>, M8),

    %% ── maps:get/3 with default ──
    <<"val">> = maps:get(<<"key">>, M1, default),
    default = maps:get(<<"missing">>, M1, default),

    %% ── Multiple binary keys in same map ──
    M9 = #{<<"one">> => 1, <<"two">> => 2, <<"three">> => 3},
    1 = maps:get(<<"one">>, M9),
    2 = maps:get(<<"two">>, M9),
    3 = maps:get(<<"three">>, M9),

    %% ── maps:take with binary key ──
    {<<"val">>, M10} = maps:take(<<"key">>, M1),
    0 = map_size(M10),
    error = maps:take(<<"nope">>, M1),

    %% ── Float key (another boxed type, same class of bug) ──
    MF = maps:put(1.5, ok, #{}),
    ok = maps:get(1.5, MF),

    %% ── Tuple keys (boxed, same bug class) ──
    MT = #{{user, 1} => alice, {user, 2} => bob},
    alice = maps:get({user, 1}, MT),
    bob = maps:get({user, 2}, MT),
    %% Runtime-constructed tuple with same atoms
    TK = {user, 1},
    alice = maps:get(TK, MT),

    %% ── Map pattern matching with binary keys ──
    #{<<"name">> := <<"Nera">>} = #{<<"name">> => <<"Nera">>},
    check_map_pattern(#{<<"status">> => <<"ok">>, <<"count">> => 5}),

    %% ── Sub-binary keys (e.g., from binary_part or binary match) ──
    FullBin = <<"key:value">>,
    <<SubKey:3/binary, $:, SubVal/binary>> = FullBin,
    MSub = #{SubKey => SubVal},
    <<"value">> = maps:get(<<"key">>, MSub),

    %% ── maps:from_list dedup with binary keys ──
    MDup = maps:from_list([{<<"k">>, 1}, {<<"k">>, 2}]),
    2 = maps:get(<<"k">>, MDup),       % last wins
    1 = map_size(MDup),                 % no duplicates

    %% ── maps:merge dedup across maps ──
    M_A2 = #{<<"x">> => old},
    M_B2 = #{<<"x">> => new},
    M_C2 = maps:merge(M_A2, M_B2),
    new = maps:get(<<"x">>, M_C2),
    1 = map_size(M_C2),

    %% ── maps:from_keys with binary keys ──
    MFK = maps:from_keys([<<"a">>, <<"b">>, <<"c">>], 0),
    0 = maps:get(<<"a">>, MFK),
    0 = maps:get(<<"b">>, MFK),
    0 = maps:get(<<"c">>, MFK),
    3 = map_size(MFK),

    %% ── maps:to_list → maps:from_list roundtrip preserves binary keys ──
    MRound = #{<<"x">> => 1, <<"y">> => 2},
    MRound2 = maps:from_list(maps:to_list(MRound)),
    1 = maps:get(<<"x">>, MRound2),
    2 = maps:get(<<"y">>, MRound2),

    %% ── Nested map with binary keys ──
    MNested = #{<<"outer">> => #{<<"inner">> => 42}},
    #{<<"inner">> := 42} = maps:get(<<"outer">>, MNested),
    42 = maps:get(<<"inner">>, maps:get(<<"outer">>, MNested)),

    io:format("ok~n"),
    ok.

check_map_pattern(#{<<"status">> := <<"ok">>, <<"count">> := N}) when N > 0 ->
    ok;
check_map_pattern(_) ->
    error(pattern_failed).
