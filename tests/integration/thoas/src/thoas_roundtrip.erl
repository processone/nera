%% Integration test for the thoas JSON library compiled with Nera.
%% Exercises encode/decode roundtrips through the public thoas API.
%%
%% thoas:encode/1 returns binary() (throws on error).
%% thoas:decode/1 returns {ok, Term} | {error, Reason}.
-module(thoas_roundtrip).
-export([test/0]).

test() ->
    %% ── Encode basic types ──
    <<"null">> = thoas:encode(null),
    <<"true">> = thoas:encode(true),
    <<"false">> = thoas:encode(false),
    <<"42">> = thoas:encode(42),
    <<"0">> = thoas:encode(0),
    <<"-1">> = thoas:encode(-1),
    <<"\"hello\"">> = thoas:encode(<<"hello">>),
    <<"\"\"">> = thoas:encode(<<>>),
    io:format("encode basic: ok~n"),

    %% ── Encode collections ──
    <<"[]">> = thoas:encode([]),
    <<"[1,2,3]">> = thoas:encode([1, 2, 3]),
    <<"[true,null,42]">> = thoas:encode([true, null, 42]),
    io:format("encode collections: ok~n"),

    %% ── Encode map + roundtrip ──
    MapJson = thoas:encode(#{<<"key">> => <<"value">>}),
    true = is_binary(MapJson),
    {ok, Map1} = thoas:decode(MapJson),
    <<"value">> = maps:get(<<"key">>, Map1),
    io:format("map roundtrip: ok~n"),

    %% ── Decode basic types ──
    {ok, null} = thoas:decode(<<"null">>),
    {ok, true} = thoas:decode(<<"true">>),
    {ok, false} = thoas:decode(<<"false">>),
    {ok, 42} = thoas:decode(<<"42">>),
    {ok, 0} = thoas:decode(<<"0">>),
    {ok, -1} = thoas:decode(<<"-1">>),
    {ok, <<"hello">>} = thoas:decode(<<"\"hello\"">>),
    {ok, <<>>} = thoas:decode(<<"\"\"">>),
    io:format("decode basic: ok~n"),

    %% ── Decode collections ──
    {ok, []} = thoas:decode(<<"[]">>),
    {ok, [1, 2, 3]} = thoas:decode(<<"[1,2,3]">>),
    {ok, [true, null, 42]} = thoas:decode(<<"[true,null,42]">>),
    io:format("decode collections: ok~n"),

    %% ── Decode object ──
    {ok, ObjMap} = thoas:decode(<<"{\"name\":\"Nera\",\"cool\":true}">>),
    <<"Nera">> = maps:get(<<"name">>, ObjMap),
    true = maps:get(<<"cool">>, ObjMap),
    io:format("decode object: ok~n"),

    %% ── Float encode/decode ──
    FloatJson = thoas:encode(3.14),
    {ok, FloatBack} = thoas:decode(FloatJson),
    true = is_float(FloatBack),
    io:format("float roundtrip: ok~n"),

    %% ── String escaping roundtrip ──
    EscJson = thoas:encode(<<"line1\nline2\ttab">>),
    {ok, <<"line1\nline2\ttab">>} = thoas:decode(EscJson),
    io:format("escape roundtrip: ok~n"),

    %% ── Decode errors ──
    {error, _} = thoas:decode(<<"{">>),
    {error, _} = thoas:decode(<<"[1,]">>),
    {error, _} = thoas:decode(<<>>),
    io:format("decode errors: ok~n"),

    %% ── Encode with atom keys ──
    AtomKeyJson = thoas:encode(#{name => <<"test">>}),
    {ok, AtomMap} = thoas:decode(AtomKeyJson),
    <<"test">> = maps:get(<<"name">>, AtomMap),
    io:format("atom keys: ok~n"),

    %% ── Nested structures (map containing list of maps) ──
    Nested = #{
        <<"users">> => [
            #{<<"name">> => <<"Alice">>, <<"age">> => 30},
            #{<<"name">> => <<"Bob">>, <<"age">> => 25}
        ],
        <<"count">> => 2
    },
    NestedJson = thoas:encode(Nested),
    {ok, Decoded} = thoas:decode(NestedJson),
    2 = maps:get(<<"count">>, Decoded),
    [User1, User2] = maps:get(<<"users">>, Decoded),
    <<"Alice">> = maps:get(<<"name">>, User1),
    30 = maps:get(<<"age">>, User1),
    <<"Bob">> = maps:get(<<"name">>, User2),
    25 = maps:get(<<"age">>, User2),
    io:format("nested roundtrip: ok~n"),

    io:format("all tests passed~n"),
    ok.
