%% EXPECT: unicode_characters_binary_ok
-module(unicode_characters_binary).
-export([test/0]).
test() ->
    <<>> = unicode:characters_to_binary([]),
    <<"hello">> = unicode:characters_to_binary("hello"),
    <<240,159,152,128>> = unicode:characters_to_binary([16#1f600]),
    <<"a",195,169,"b">> = unicode:characters_to_binary(["a", <<195>>, [<<169>>, $b]]),
    {error, <<"a">>, [16#d800, $b]} = unicode:characters_to_binary([$a,16#d800,$b]),
    {error, <<"a">>, <<255,$b>>} = unicode:characters_to_binary(<<$a,255,$b>>),
    {incomplete, <<"a">>, <<195>>} = unicode:characters_to_binary(<<$a,195>>),
    {error, <<"a">>, [<<195>>, [[<<255>>, $b]]]} = unicode:characters_to_binary([$a, <<195>>, [<<255>>, $b]]),
    {incomplete, <<"a">>, <<195>>} = unicode:characters_to_binary([$a, <<195>>]),
    {error, <<>>, [<<240,159,255>>, [$b]]} = unicode:characters_to_binary([<<240>>, <<159>>, <<255>>, $b]),
    {incomplete, <<>>, <<192>>} = unicode:characters_to_binary(<<192>>),
    {incomplete, <<>>, <<237,160>>} = unicode:characters_to_binary(<<237,160>>),
    {error, <<>>, <<240,159,255>>} = unicode:characters_to_binary([<<240>>,<<159>>,<<255>>]),
    ok = try unicode:characters_to_binary([atom]) of _ -> error(unexpected) catch error:badarg -> ok end,
    io:format("unicode_characters_binary_ok~n").
