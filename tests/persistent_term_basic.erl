%% Tests for persistent_term module.
-module(persistent_term_basic).
-export([test/0]).

test() ->
    %% put and get
    ok = persistent_term:put(my_key, my_value),
    my_value = persistent_term:get(my_key),

    %% get with default — key exists
    my_value = persistent_term:get(my_key, default_val),

    %% get with default — key missing
    default_val = persistent_term:get(missing_key, default_val),

    %% overwrite existing key
    ok = persistent_term:put(my_key, updated_value),
    updated_value = persistent_term:get(my_key),

    %% integer key and value
    ok = persistent_term:put(42, {hello, world}),
    {hello, world} = persistent_term:get(42),

    %% tuple key
    ok = persistent_term:put({app, config}, [1, 2, 3]),
    [1, 2, 3] = persistent_term:get({app, config}),

    %% erase
    true = persistent_term:erase(42),
    %% after erase, get/2 returns default
    not_found = persistent_term:get(42, not_found),

    %% info returns a map with count and memory
    #{count := Count, memory := _Mem} = persistent_term:info(),
    true = Count >= 2,

    %% complex terms as values
    ok = persistent_term:put(complex, #{a => [1,2,3], b => <<"binary">>}),
    #{a := [1,2,3], b := <<"binary">>} = persistent_term:get(complex),

    %% clean up
    true = persistent_term:erase(my_key),
    true = persistent_term:erase({app, config}),
    true = persistent_term:erase(complex),

    io:format("persistent_term_basic: all tests passed~n"),
    ok.
