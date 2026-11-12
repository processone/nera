%% Adapted from OTP compiler/test/map_SUITE.erl (Apache 2.0)
%% Tests map construction and pattern matching.
%% Note: map update syntax (M#{k := v}) is not yet supported in parser.
-module(match_map).
-export([test/0]).

test() ->
    %% Map construction
    M1 = #{a => 1, b => 2, c => 3},

    %% Map access via maps module
    1 = maps:get(a, M1),
    2 = maps:get(b, M1),
    3 = maps:get(c, M1),

    %% maps:put to update existing key
    M2 = maps:put(a, 10, M1),
    10 = maps:get(a, M2),
    2 = maps:get(b, M2),

    %% maps:put to add new key
    M3 = maps:put(d, 4, M1),
    4 = maps:get(d, M3),

    %% Map pattern matching in function heads
    1 = get_a(#{a => 1, b => 2}),
    42 = get_a(#{a => 42}),

    %% Map size
    3 = map_size(M1),
    4 = map_size(M3),
    0 = map_size(#{}),

    %% maps:is_key
    true = maps:is_key(a, M1),
    false = maps:is_key(d, M1),

    %% maps:put
    M4 = maps:put(x, 99, M1),
    99 = maps:get(x, M4),

    %% Empty map
    M5 = #{},
    0 = map_size(M5),

    %% Map with different key types
    M6 = #{1 => one, hello => world},
    one = maps:get(1, M6),
    world = maps:get(hello, M6),

    %% Guard with is_map
    true = is_a_map(#{}),
    true = is_a_map(#{a => 1}),
    false = is_a_map(not_a_map),
    false = is_a_map([]),

    io:format("ok~n"),
    ok.

get_a(#{a := V}) -> V.

is_a_map(X) when is_map(X) -> true;
is_a_map(_) -> false.
