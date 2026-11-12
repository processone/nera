%% EXPECT: map_bound_value_vars_ok
%% Regression: a map pattern value that is an already-bound variable is
%% compared with =:= like BEAM, instead of overwriting the bound variable.
%% In = and ?=, later keys and binary sizes still read that variable, and a
%% bound binary tail stays readable on every path.
-module(map_bound_value_vars).
-export([test/0]).

test() ->
    Owner = id(self()),
    Other = id(spawn(fun() -> ok end)),

    %% The same variable twice in one map pattern.
    {same, Owner} = same_owner(id(#{1 => Owner, 3 => Owner})),
    {different, Owner, Other} = same_owner(id(#{1 => Owner, 3 => Other})),
    {different, 1, 1.0} = same_owner(id(#{1 => 1, 3 => 1.0})),
    {same, 2} = nested(id(#{1 => #{owner => 2}, 3 => 2})),
    different = nested(id(#{1 => #{owner => 2}, 3 => 4})),

    %% A variable bound by an earlier argument; the next clause still sees
    %% the caller's value.
    {mine, Owner} = owned_by(Owner, id(#{1 => Owner})),
    {not_mine, Owner, Other} = owned_by(Owner, id(#{1 => Other})),
    {second, Owner} = owner_after_guard(Owner, id(#{1 => Other})),
    %% A variable bound in a map value, compared later in the same head.
    {mine, Owner} = owned_by_later(id(#{1 => Owner}), Owner),
    {not_mine, Other, Owner} = owned_by_later(id(#{1 => Other}), Owner),
    {same, 5} = tuple_then_map(id({5, #{k => 5}})),
    {different, 5, 6} = tuple_then_map(id({5, #{k => 6}})),
    {same, 5} = map_then_tuple(id({#{k => 5}, 5})),
    {different, 5, 6} = map_then_tuple(id({#{k => 5}, 6})),
    {owner, Owner} = guarded(id(#{1 => Owner, 3 => Owner})),
    other = guarded(id(#{1 => Owner, 3 => Other})),

    %% A case on a variable bound before the case.
    StreamID = id(1),
    Streams = id(#{1 => Owner, 3 => Other}),
    mine = stream_owner(Streams, StreamID, Owner),
    not_mine = stream_owner(Streams, StreamID, Other),
    missing = stream_owner(Streams, 2, Owner),
    not_mine = case id(#{owner => Other}) of
        #{owner := Owner} -> mine;
        _ -> not_mine
    end,
    %% The failed case must not rebind Owner.
    Owner = id(self()),
    ok = try
        case id(#{owner => Other}) of
            #{owner := Owner} -> mine
        end
    catch
        error:{case_clause, Unmatched} -> true = Unmatched =:= #{owner => Other}, ok
    end,

    not_mine = maybe
        #{owner := Owner} ?= id(#{owner => Other}),
        mine
    else
        _ -> not_mine
    end,

    %% A match expression with a bound variable raises badmatch, and the
    %% variable keeps its value in the handler.
    {ok, Owner} = bound_match(Owner, Other),

    %% In = and ?=, a bound variable compared in a map value is then read
    %% as a later key or binary size while the pattern is still matching.
    Streams2 = id(#{active => 1, 1 => stream1, 3 => stream3}),
    stream1 = active_stream(1, Streams2),
    {badmatch, Streams2} = active_stream(3, Streams2),
    {ok, stream1} = maybe_active_stream(1, Streams2),
    no = maybe_active_stream(3, Streams2),
    Frame = id(#{length => 2, data => <<"hi!">>}),
    <<"hi">> = frame_body(2, Frame),
    {badmatch, Frame} = frame_body(3, Frame),
    {ok, <<"hi">>} = maybe_frame_body(2, Frame),
    no = maybe_frame_body(3, Frame),
    %% The same holds when the earlier occurrence is outside the map.
    stream3 = paired_stream(3, id({3, Streams2})),
    {badmatch, {1, Streams2}} = paired_stream(3, id({1, Streams2})),
    <<"hi">> = paired_body(2, id({2, <<"hi!">>})),

    %% receive skips messages whose map value differs.
    self() ! #{owner => Other, n => 1},
    self() ! #{owner => Owner, n => 2},
    2 = receive #{owner := Owner, n := N} -> N after 1000 -> timeout end,
    #{n := 1} = receive Message -> Message after 1000 -> timeout end,

    %% A bound binary tail (kept as a deferred sub-binary) is both compared
    %% as a map value and used as a key, in every matching construct.
    Field = id(<<1, "host">>),
    Headers = id(#{name => <<"host">>, <<"host">> => <<"example.org">>}),
    Mismatch = id(#{name => <<"path">>, <<"host">> => <<"example.org">>}),
    <<"example.org">> = tail_case(Field, Headers),
    no = tail_case(Field, Mismatch),
    <<"example.org">> = tail_match(Field, Headers),
    badmatch = tail_match(Field, Mismatch),
    {ok, <<"example.org">>} = tail_maybe(Field, Headers),
    no = tail_maybe(Field, Mismatch),
    <<"example.org">> = tail_receive(Field, Headers),
    no = tail_receive(Field, Mismatch),
    <<"example.org">> = tail_maybe_else(Field, Headers),
    no = tail_maybe_else(Field, Mismatch),
    %% The tail stays readable on every path and after the match.
    {<<"example.org">>, <<"host">>} = tail_key_first(Field, Headers),
    {<<"example.org">>, <<"host">>} = tail_after_case(Field, Headers),
    {<<"host">>, <<"host">>} = tail_after_maybe(Field, Headers),
    {{no, <<"host">>}, <<"host">>} = tail_after_maybe(Field, id(#{})),
    same = tail_guard_then_compare(Field, id(<<"host">>)),
    no = tail_guard_then_compare(Field, id(<<"path">>)),
    %% A receive whose after body reads the tail: with the message already
    %% queued, on timeout, and with the tail read after the receive.
    {matched, <<"host">>} = tail_receive_after(Field, Headers, queued),
    {<<"host">>, <<"host">>} = tail_receive_after(Field, Headers, empty),
    %% A branch that reads the tail must not change it for the code after the
    %% branch, whether it ran or not.
    {got, matched, <<"host">>} = tail_after_receive_after(Field, Headers),
    {other, matched, <<"host">>} = tail_after_if_body(Field, Headers),
    {map, matched, <<"host">>} = tail_after_if_guard(Field, Headers),
    {host, matched, <<"host">>} = tail_after_raising_guard(Field, Headers),
    {caught, matched, <<"host">>} = tail_after_try_raise(Field, Headers),
    {other, matched, <<"host">>} = tail_after_try_of(Field, Headers),
    {<<"host">>, matched, <<"host">>} = tail_after_catch(Field, Headers, throw),
    {ok, matched, <<"host">>} = tail_after_catch(Field, Headers, return),
    {exit, matched, <<"host">>} = tail_after_catch_expr(Field, Headers),
    {false, matched, <<"host">>} = tail_after_andalso(Field, Headers),
    {true, matched, <<"host">>} = tail_after_orelse(Field, Headers),

    %% Fun heads and generators shadow outer variables, as in BEAM.
    Key = id(k),
    Fun = fun(#{k := Key}) -> Key end,
    v = Fun(id(#{k => v})),
    [a, b] = [Key || #{k := Key} <- id([#{k => a}, #{k => b}])],
    k = Key,

    io:format("map_bound_value_vars_ok~n"),
    ok.

same_owner(#{1 := Pid, 3 := Pid}) -> {same, Pid};
same_owner(#{1 := A, 3 := B}) -> {different, A, B}.

nested(#{1 := #{owner := Pid}, 3 := Pid}) -> {same, Pid};
nested(_) -> different.

owned_by(Pid, #{1 := Pid}) -> {mine, Pid};
owned_by(Pid, #{1 := Found}) -> {not_mine, Pid, Found}.

%% If the map value overwrote Pid, the failing guard would hand the next
%% clause the map value instead of the caller's.
owner_after_guard(Pid, #{1 := Pid}) when is_integer(Pid) -> first;
owner_after_guard(Pid, _) -> {second, Pid}.

owned_by_later(#{1 := Pid}, Pid) -> {mine, Pid};
owned_by_later(#{1 := Found}, Pid) -> {not_mine, Found, Pid}.

tuple_then_map({X, #{k := X}}) -> {same, X};
tuple_then_map({X, #{k := Y}}) -> {different, X, Y}.

map_then_tuple({#{k := X}, X}) -> {same, X};
map_then_tuple({#{k := X}, Y}) -> {different, X, Y}.

guarded(#{1 := Pid, 3 := Pid}) when is_pid(Pid) -> {owner, Pid};
guarded(_) -> other.

stream_owner(Streams, StreamID, Pid) ->
    case Streams of
        #{StreamID := Pid} -> mine;
        #{StreamID := _} -> not_mine;
        _ -> missing
    end.

bound_match(Owner, Other) ->
    try
        #{owner := Owner} = id(#{owner => Other}),
        matched
    catch
        error:{badmatch, Unmatched} -> true = Unmatched =:= #{owner => Other}, {ok, Owner}
    end.

active_stream(StreamID, Streams) ->
    try
        #{active := StreamID, StreamID := Stream} = Streams,
        Stream
    catch
        error:{badmatch, Value} -> {badmatch, Value}
    end.

maybe_active_stream(StreamID, Streams) ->
    maybe
        #{active := StreamID, StreamID := Stream} ?= Streams,
        {ok, Stream}
    else
        _ -> no
    end.

frame_body(Len, Frame) ->
    try
        #{length := Len, data := <<Body:Len/binary, _/binary>>} = Frame,
        Body
    catch
        error:{badmatch, Value} -> {badmatch, Value}
    end.

maybe_frame_body(Len, Frame) ->
    maybe
        #{length := Len, data := <<Body:Len/binary, _/binary>>} ?= Frame,
        {ok, Body}
    else
        _ -> no
    end.

paired_stream(StreamID, Pair) ->
    try
        {StreamID, #{StreamID := Stream}} = Pair,
        Stream
    catch
        error:{badmatch, Value} -> {badmatch, Value}
    end.

paired_body(Len, Pair) ->
    {Len, <<Body:Len/binary, _/binary>>} = Pair,
    Body.

tail_case(<<_, Name/binary>>, Headers) ->
    case Headers of
        #{name := Name, Name := Value} -> Value;
        _ -> no
    end.

tail_match(<<_, Name/binary>>, Headers) ->
    try
        #{name := Name, Name := Value} = Headers,
        Value
    catch
        error:{badmatch, _} -> badmatch
    end.

tail_maybe(<<_, Name/binary>>, Headers) ->
    maybe
        #{name := Name, Name := Value} ?= Headers,
        {ok, Value}
    else
        _ -> no
    end.

tail_receive(<<_, Name/binary>>, Headers) ->
    self() ! Headers,
    receive
        #{name := Name, Name := Value} -> Value
    after 0 ->
        receive Headers -> no end
    end.

tail_maybe_else(<<_, Name/binary>>, Headers) ->
    maybe
        {ok, _} ?= Headers,
        unexpected
    else
        #{name := Name, Name := Value} -> Value;
        _ -> no
    end.

tail_key_first(<<_, Name/binary>>, Headers) ->
    #{Name := Value, name := Name} = Headers,
    {Value, Name}.

tail_after_case(<<_, Name/binary>>, Headers) ->
    Value = case Headers of
        #{Name := Found} -> Found;
        _ -> no
    end,
    {Value, Name}.

%% The success path and the else path both read Name, and so does the code
%% after the maybe.
tail_after_maybe(<<_, Name/binary>>, Headers) ->
    Result = maybe
        #{name := Name} ?= Headers,
        id(Name)
    else
        _ -> {no, Name}
    end,
    {Result, Name}.

%% The second clause's guard reads Name before the first clause compares it.
tail_guard_then_compare(<<_, Name/binary>>, Value) ->
    case Value of
        Name -> same;
        _ when Value =:= Name -> guard;
        _ -> no
    end.

tail_receive_after(<<_, Name/binary>>, Headers, Mailbox) ->
    case Mailbox of
        queued -> self() ! Headers;
        empty -> ok
    end,
    Result = receive
        #{name := Name} -> matched
    after 0 ->
        Name
    end,
    {Result, Name}.

%% Each helper below reads Name in a branch, then compares it in a map
%% pattern of the same function and returns it.
tail_after_receive_after(<<_, Name/binary>>, Headers) ->
    self() ! go,
    Result = receive go -> got after 0 -> id(Name) end,
    Compared = case Headers of
        #{name := Name} -> matched;
        _ -> no
    end,
    {Result, Compared, Name}.

tail_after_if_body(<<_, Name/binary>>, Headers) ->
    Result = if is_atom(Headers) -> id(Name); true -> other end,
    Compared = case Headers of
        #{name := Name} -> matched;
        _ -> no
    end,
    {Result, Compared, Name}.

tail_after_if_guard(<<_, Name/binary>>, Headers) ->
    Result = if
        is_map(Headers) -> map;
        Name =:= <<"host">> -> host;
        true -> other
    end,
    Compared = case Headers of
        #{name := Name} -> matched;
        _ -> no
    end,
    {Result, Compared, Name}.

tail_after_raising_guard(<<_, Name/binary>>, Headers) ->
    Result = if
        element(1, Headers) =:= Name -> tuple;
        Name =:= <<"host">> -> host;
        true -> other
    end,
    Compared = case Headers of
        #{name := Name} -> matched;
        _ -> no
    end,
    {Result, Compared, Name}.

tail_after_try_raise(<<_, Name/binary>>, Headers) ->
    Result = try
        1 = id(2),
        id(Name)
    catch
        error:{badmatch, 2} -> caught
    end,
    Compared = case Headers of
        #{name := Name} -> matched;
        _ -> no
    end,
    {Result, Compared, Name}.

tail_after_try_of(<<_, Name/binary>>, Headers) ->
    Result = try id(Headers) of
        none -> id(Name);
        _ -> other
    catch
        _:_ -> caught
    end,
    Compared = case Headers of
        #{name := Name} -> matched;
        _ -> no
    end,
    {Result, Compared, Name}.

tail_after_catch(<<_, Name/binary>>, Headers, How) ->
    Result = try
        case How of
            throw -> throw(failed);
            return -> ok
        end
    catch
        throw:failed -> id(Name)
    end,
    Compared = case Headers of
        #{name := Name} -> matched;
        _ -> no
    end,
    {Result, Compared, Name}.

tail_after_catch_expr(<<_, Name/binary>>, Headers) ->
    Result = case catch begin 1 = id(2), id(Name) end of
        {'EXIT', _} -> exit;
        _ -> other
    end,
    Compared = case Headers of
        #{name := Name} -> matched;
        _ -> no
    end,
    {Result, Compared, Name}.

tail_after_andalso(<<_, Name/binary>>, Headers) ->
    Result = is_atom(Headers) andalso Name =:= <<"host">>,
    Compared = case Headers of
        #{name := Name} -> matched;
        _ -> no
    end,
    {Result, Compared, Name}.

tail_after_orelse(<<_, Name/binary>>, Headers) ->
    Result = is_map(Headers) orelse Name =:= <<"host">>,
    Compared = case Headers of
        #{name := Name} -> matched;
        _ -> no
    end,
    {Result, Compared, Name}.

id(Value) -> Value.
