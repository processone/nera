%% EXPECT: pattern_expr_warnings_ok
-module(pattern_expr_warnings).
-export([test/0]).
-record(packet, {data}).

%% Regression: map keys and binary sizes are reads, not unused bindings.
%% The lookup, prefix and tag shapes reproduce real HTTP stream/router code.
test() ->
    stream = lookup(id(#{7 => stream}), id(7)),
    <<"abc">> = prefix(id(<<"abc/rest">>), id(3)),
    <<"tag">> = tag(id(<<"tag!">>)),
    <<"abc">> = frame(id(<<3, "abc">>)),
    <<"abc">> = frame_case(id(<<3, "abc">>)),
    <<"abc">> = frame_match(id(<<3, "abc">>)),
    <<"abc">> = frame_nested(id({packet, [#packet{data = <<3, "abc">>}]})),
    <<"abc">> = frame_fun(id(<<3, "abc">>)),
    <<"abc">> = frame_receive(id(<<3, "abc">>)),
    <<"abc">> = frame_try(id(<<3, "abc">>)),
    <<"abc">> = frame_catch(id(<<3, "abc">>)),
    <<"abc">> = frame_chain(id(<<3, "abc">>)),
    stream = map_chain(id(#{entry => 7, 7 => stream})),
    stream = unused_shadow(id(7), id(#{entry => ignored, 7 => stream})),
    ok = unused_controls(id(#{entry => <<3, "abc">>}), id(ignored)),
    io:format("pattern_expr_warnings_ok~n"),
    ok.

lookup(Streams, StreamID) ->
    case Streams of #{StreamID := Stream} -> Stream end.

prefix(Segment, Pos) ->
    case Segment of <<Prefix:Pos/binary, _/binary>> -> Prefix end.

tag(Tag) ->
    Length = byte_size(Tag) - 1,
    <<Value:Length/binary, _>> = Tag,
    Value.

frame(<<Length, Payload:Length/binary>>) -> Payload.

frame_case(Bin) ->
    case Bin of <<Length, Payload:Length/binary>> -> Payload end.

frame_match(Bin) ->
    <<Length, Payload:Length/binary>> = Bin,
    Payload.

frame_nested({packet, [#packet{data = Frame = <<Length, Payload:Length/binary>>}]}) ->
    <<_:8, _/binary>> = Frame,
    Payload.

frame_fun(Bin) ->
    F = fun(<<Length, Payload:Length/binary>>) -> Payload end,
    F(Bin).

frame_receive(Bin) ->
    self() ! Bin,
    receive <<Length, Payload:Length/binary>> -> Payload end.

frame_try(Bin) ->
    try Bin of <<Length, Payload:Length/binary>> -> Payload catch _:_ -> error end.

frame_catch(Bin) ->
    try throw({packet, Bin}) catch throw:{packet, <<Length, Payload:Length/binary>>} -> Payload end.

frame_chain(Bin) ->
    Frame = <<Length, Payload:Length/binary>> = Bin,
    <<_:8, _/binary>> = Frame,
    Payload.

map_chain(Map) ->
    #{Key := Value} = #{entry := Key} = Map,
    Value.

%% A key reads the outer input, not the shadowing value in the same fun head.
unused_shadow(ShadowedKey, Map) ->
    F = fun(#{entry := ShadowedKey, ShadowedKey := Value}) -> Value end,
    F(Map).

%% These bindings really are unused; warning analysis must still report them.
unused_controls(Map, UnusedArgument) ->
    #{entry := <<UnusedLength, UnusedBinaryValue:UnusedLength/binary>> = UnusedMapValue} = Map,
    ok.

id(X) -> X.
