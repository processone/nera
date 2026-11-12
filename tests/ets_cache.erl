%% Cache with TTL eviction inspired by Armstrong's practical ETS patterns.
%% Exercises: insert, lookup, select_delete with guards, info/2,
%%            delete_all_objects, insert_new, update_element.
-module(ets_cache).
-export([test/0]).

test() ->
    test_basic_cache(),
    test_evict_old_entries(),
    test_cache_update(),
    test_lru_simulation(),
    test_cache_stats(),
    ok.

%% Basic cache: {Key, Value, Timestamp}
test_basic_cache() ->
    T = ets:new(cache, [set, public]),

    %% Cache some values with timestamps
    ets:insert(T, {user_1, "Alice", 1000}),
    ets:insert(T, {user_2, "Bob", 1001}),
    ets:insert(T, {user_3, "Carol", 1002}),

    %% Cache hit
    [{user_1, "Alice", 1000}] = ets:lookup(T, user_1),

    %% Cache miss
    [] = ets:lookup(T, user_99),

    3 = ets:info(T, size),

    ets:delete(T),
    ok.

%% Evict entries older than a threshold using select_delete
test_evict_old_entries() ->
    T = ets:new(ttl_cache, [set, public]),

    %% Insert entries with varying timestamps
    ets:insert(T, {a, val_a, 100}),
    ets:insert(T, {b, val_b, 200}),
    ets:insert(T, {c, val_c, 300}),
    ets:insert(T, {d, val_d, 400}),
    ets:insert(T, {e, val_e, 500}),

    5 = ets:info(T, size),

    %% Evict entries with timestamp < 300 ($1 bound at position 3 in pattern)
    Deleted = ets:select_delete(T, [{{'_', '_', '$1'}, [{'<', '$1', 300}], [true]}]),
    2 = Deleted,

    %% Verify survivors
    3 = ets:info(T, size),
    [{c, val_c, 300}] = ets:lookup(T, c),
    [{d, val_d, 400}] = ets:lookup(T, d),
    [{e, val_e, 500}] = ets:lookup(T, e),

    %% Evicted entries are gone
    [] = ets:lookup(T, a),
    [] = ets:lookup(T, b),

    %% Evict more: timestamp < 450 (both c=300 and d=400 match)
    2 = ets:select_delete(T, [{{'_', '_', '$1'}, [{'<', '$1', 450}], [true]}]),
    1 = ets:info(T, size),
    [] = ets:lookup(T, c),
    [] = ets:lookup(T, d),

    ets:delete(T),
    ok.

%% Update cached values and their timestamps
test_cache_update() ->
    T = ets:new(upd_cache, [set, public]),

    %% Initial insert
    ets:insert(T, {session_1, "data_v1", 1000}),
    [{session_1, "data_v1", 1000}] = ets:lookup(T, session_1),

    %% Update value (overwrite with new timestamp)
    ets:insert(T, {session_1, "data_v2", 2000}),
    [{session_1, "data_v2", 2000}] = ets:lookup(T, session_1),

    %% Still only one entry
    1 = ets:info(T, size),

    %% Use update_element to just change the timestamp
    ets:update_element(T, session_1, {3, 3000}),
    [{session_1, "data_v2", 3000}] = ets:lookup(T, session_1),

    ets:delete(T),
    ok.

%% Simulate LRU-like behavior: track access counts and evict least-used
%% {Key, Value, AccessCount}
test_lru_simulation() ->
    T = ets:new(lru, [set, public]),

    %% Populate cache
    ets:insert(T, [
        {k1, "val1", 0},
        {k2, "val2", 0},
        {k3, "val3", 0},
        {k4, "val4", 0},
        {k5, "val5", 0}
    ]),

    %% Simulate access pattern: k1 and k3 are popular
    cache_get(T, k1),
    cache_get(T, k1),
    cache_get(T, k1),
    cache_get(T, k3),
    cache_get(T, k3),
    cache_get(T, k2),

    %% Verify access counts
    [{k1, "val1", 3}] = ets:lookup(T, k1),
    [{k2, "val2", 1}] = ets:lookup(T, k2),
    [{k3, "val3", 2}] = ets:lookup(T, k3),
    [{k4, "val4", 0}] = ets:lookup(T, k4),

    %% Evict entries with access count < 2 ($1 bound at position 3)
    Evicted = ets:select_delete(T, [{{'_', '_', '$1'}, [{'<', '$1', 2}], [true]}]),
    3 = Evicted,  %% k2(1), k4(0), k5(0)

    %% Survivors are the popular ones
    2 = ets:info(T, size),
    true = ets:member(T, k1),
    true = ets:member(T, k3),
    false = ets:member(T, k2),
    false = ets:member(T, k4),
    false = ets:member(T, k5),

    ets:delete(T),
    ok.

%% Cache with separate stats tracking
test_cache_stats() ->
    Cache = ets:new(data_cache, [set, public]),
    Stats = ets:new(cache_stats, [set, public]),

    %% Initialize stats
    ets:insert(Stats, {hits, 0}),
    ets:insert(Stats, {misses, 0}),
    ets:insert(Stats, {inserts, 0}),

    %% Populate cache
    cache_put(Cache, Stats, key1, "value1"),
    cache_put(Cache, Stats, key2, "value2"),
    cache_put(Cache, Stats, key3, "value3"),

    %% Hits
    {ok, "value1"} = cache_lookup(Cache, Stats, key1),
    {ok, "value2"} = cache_lookup(Cache, Stats, key2),
    {ok, "value1"} = cache_lookup(Cache, Stats, key1),

    %% Misses
    miss = cache_lookup(Cache, Stats, unknown1),
    miss = cache_lookup(Cache, Stats, unknown2),

    %% Verify stats
    [{hits, 3}] = ets:lookup(Stats, hits),
    [{misses, 2}] = ets:lookup(Stats, misses),
    [{inserts, 3}] = ets:lookup(Stats, inserts),

    ets:delete(Cache),
    ets:delete(Stats),
    ok.

%% --- Helpers ---

%% Simulate a cache get that increments access count
cache_get(T, Key) ->
    ets:update_counter(T, Key, {3, 1}).

%% Cache put with stats tracking
cache_put(Cache, Stats, Key, Value) ->
    ets:insert(Cache, {Key, Value}),
    ets:update_counter(Stats, inserts, 1).

%% Cache lookup with stats tracking
cache_lookup(Cache, Stats, Key) ->
    case ets:lookup(Cache, Key) of
        [{_, Value}] ->
            ets:update_counter(Stats, hits, 1),
            {ok, Value};
        [] ->
            ets:update_counter(Stats, misses, 1),
            miss
    end.
