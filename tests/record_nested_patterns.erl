%% EXPECT: record_nested_patterns_ok
%% Regression: Cowboy HTTP/2 2.20.0 matches stream records inside stream maps.
-module(record_nested_patterns).
-export([test/0]).

-record(stream, {status = running, flow = 0, state}).
-record(connection, {streams = #{}}).

test() ->
    StreamID = id(1),
    Stream = id(#stream{flow = 128, state = handler}),
    Streams = id(#{StreamID => Stream}),
    {Stream, 128, handler} = lookup(Streams, StreamID),
    missing = lookup(Streams, 3),
    missing = lookup(id(not_a_map), StreamID),
    missing = lookup(id(#{1 => {other, running, 128, handler}}), StreamID),
    missing = lookup(id(#{1 => {stream, running, 128}}), StreamID),
    missing = lookup(id(#{1 => Stream#stream{status = stopping}}), StreamID),
    %% Omitted fields in patterns are wildcards, not record defaults.
    128 = flow(id(#{1 => Stream#stream{status = stopping}})),
    missing = flow(id(#{})),
    %% Repeated bindings still require equality through the nested record.
    equal = same_flow(id(#{1 => Stream, 3 => Stream})),
    different = same_flow(id(#{1 => Stream, 3 => Stream#stream{flow = 256}})),
    %% Traverse maps under records, tuples, lists and aliases too.
    128 = nested(id({[#connection{streams = Streams}]})),
    %% The same normalization is used in matches, closures and generators.
    #{StreamID := #stream{flow = BoundFlow}} = Streams,
    128 = BoundFlow,
    F = fun(#{1 := #stream{flow = Flow}}) -> Flow; (_) -> missing end,
    128 = F(Streams),
    missing = F(#{}),
    [128] = [Flow || #{1 := #stream{flow = Flow}} <- [#{}, Streams]],
    self() ! Streams,
    receive
        #{1 := #stream{flow = ReceivedFlow}} -> 128 = ReceivedFlow
    after 1000 -> error(receive_timeout)
    end,
    128 = try id(Streams) of
        #{1 := #stream{flow = TryFlow}} -> TryFlow
    catch error:Reason -> error(Reason)
    end,
    %% A string-prefix tail can itself contain record patterns.
    128 = prefixed(id("stream:" ++ [Stream])),
    missing = prefixed(id("other:" ++ [Stream])),
    missing = prefixed(id("stream:" ++ [not_a_stream])),
    io:format("record_nested_patterns_ok~n"),
    ok.

lookup(Streams, StreamID) ->
    case Streams of
        #{StreamID := Stream = #stream{status = running, flow = Flow, state = State}} ->
            {Stream, Flow, State};
        _ -> missing
    end.

flow(#{1 := #stream{flow = Flow}}) -> Flow;
flow(_) -> missing.

same_flow(#{1 := #stream{flow = Flow}, 3 := #stream{flow = Flow}}) -> equal;
same_flow(_) -> different.

nested({[Connection = #connection{streams = #{1 := #stream{flow = Flow}}}]}) ->
    #connection{} = Connection,
    Flow.

prefixed("stream:" ++ [#stream{flow = Flow}]) -> Flow;
prefixed(_) -> missing.

id(Value) -> Value.
