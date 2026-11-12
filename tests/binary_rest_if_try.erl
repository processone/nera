%% EXPECT: binary_rest_if_try_ok
%% Regression: a deferred binary tail (Payload in <<_, Payload/binary>>) read
%% in one branch of an if or a try ... of stays readable in the other branches
%% and after the expression, as in BEAM. Before the fix, each function below
%% made the compiler panic in code generation, or a later read returned an
%% undefined value. The same fixture runs on BEAM.
-module(binary_rest_if_try).
-export([test/0]).

test() ->
    %% Final text frames: an opcode byte (0x81), then the payload.
    Ping = id(<<16#81, "ping">>),
    Pong = id(<<16#81, "pong">>),
    Text = id(#{mode => text}),
    Raw = id(raw),

    %% if: the branch that reads the payload is taken or skipped, then the
    %% payload is read again after the if.
    {{text, <<"ping">>}, <<"ping">>} = if_then_read(Ping, Text),
    {raw, <<"ping">>} = if_then_read(Ping, Raw),
    %% A later branch returns the payload after an earlier one read it.
    {text, <<"ping">>} = if_other_branch(Ping, Text),
    <<"ping">> = if_other_branch(Ping, Raw),
    %% A later guard reads the payload after an earlier body read it.
    {{text, <<"ping">>}, <<"ping">>} = if_later_guard(Ping, Text),
    {ping, <<"ping">>} = if_later_guard(Ping, Raw),
    {other, <<"pong">>} = if_later_guard(Pong, Raw),
    {{text, <<"ping">>}, <<"ping">>} = if_nested(Ping, Text),
    {raw, <<"ping">>} = if_nested(Ping, Raw),
    {{text, <<"ping">>}, skipped, <<"ping">>} = if_twice(Ping, Text),
    {raw, {text, <<"ping">>}, <<"ping">>} = if_twice(Ping, Raw),
    %% The payload is compared in a pattern after the if.
    {same, <<"ping">>} = if_then_compare(Ping, Text),
    {same, <<"ping">>} = if_then_compare(Ping, Raw),
    {different, <<"pong">>} = if_then_compare(Pong, Text),

    %% try ... of: the clause that returns the payload is taken or skipped,
    %% then the payload is read again after the try.
    {<<"ping">>, <<"ping">>} = try_then_read(Ping, Text),
    {raw, <<"ping">>} = try_then_read(Ping, Raw),
    %% A later clause returns the payload after an earlier one read it.
    {text, <<"ping">>} = try_other_clause(Ping, Text),
    <<"ping">> = try_other_clause(Ping, Raw),
    %% Only the handler reads the payload.
    {accepted, <<"ping">>} = try_handler_reads(Ping, Text),
    {{text, <<"ping">>}, <<"ping">>} = try_handler_reads(Ping, Raw),
    %% The after body reads the payload too.
    {{text, <<"ping">>}, <<"ping">>, <<"ping">>} = try_after_reads(Ping, Text),
    {raw, <<"ping">>, <<"ping">>} = try_after_reads(Ping, Raw),
    %% A guard of an earlier clause reads the payload.
    {expected, <<"ping">>} = try_later_clause(Ping, id(#{expect => <<"ping">>})),
    {{text, <<"ping">>}, <<"ping">>} = try_later_clause(Ping, id(#{expect => <<"pong">>})),
    {raw, <<"ping">>} = try_later_clause(Ping, Raw),
    {<<"ping">>, <<"ping">>} = try_inside_if(Ping, Text),
    {raw, <<"ping">>} = try_inside_if(Ping, Raw),
    {{text, <<"ping">>}, skipped, <<"ping">>} = try_twice(Ping, Text),
    {raw, {text, <<"ping">>}, <<"ping">>} = try_twice(Ping, Raw),
    %% The payload is compared in a pattern after the try.
    {same, <<"ping">>} = try_then_compare(Ping, Text),
    {same, <<"ping">>} = try_then_compare(Ping, Raw),
    {different, <<"pong">>} = try_then_compare(Pong, Text),

    io:format("binary_rest_if_try_ok~n"),
    ok.

if_then_read(<<_, Payload/binary>>, Opts) ->
    Result = if is_map(Opts) -> decode(Payload); true -> raw end,
    {Result, Payload}.

if_other_branch(<<_, Payload/binary>>, Opts) ->
    if is_map(Opts) -> decode(Payload); true -> Payload end.

if_later_guard(<<_, Payload/binary>>, Opts) ->
    Result = if
        is_map(Opts) -> decode(Payload);
        Payload =:= <<"ping">> -> ping;
        true -> other
    end,
    {Result, Payload}.

if_nested(<<_, Payload/binary>>, Opts) ->
    Result = if
        is_map(Opts) ->
            if map_size(Opts) =:= 0 -> empty; true -> decode(Payload) end;
        true -> raw
    end,
    {Result, Payload}.

if_twice(<<_, Payload/binary>>, Opts) ->
    First = if is_map(Opts) -> decode(Payload); true -> raw end,
    Second = if is_atom(Opts) -> decode(Payload); true -> skipped end,
    {First, Second, Payload}.

if_then_compare(<<_, Payload/binary>>, Opts) ->
    Decoded = if is_map(Opts) -> decode(Payload); true -> raw end,
    Result = case id({<<"ping">>, Decoded}) of
        {Payload, _} -> same;
        _ -> different
    end,
    {Result, Payload}.

try_then_read(<<_, Payload/binary>>, Opts) ->
    Result = try id(Opts) of
        #{} -> Payload;
        _ -> raw
    catch
        _:_ -> invalid
    end,
    {Result, Payload}.

try_other_clause(<<_, Payload/binary>>, Opts) ->
    try id(Opts) of
        #{} -> decode(Payload);
        _ -> Payload
    catch
        _:_ -> invalid
    end.

try_handler_reads(<<_, Payload/binary>>, Opts) ->
    Result = try validate(Opts) of
        ok -> accepted
    catch
        error:badarg -> decode(Payload)
    end,
    {Result, Payload}.

try_after_reads(<<_, Payload/binary>>, Opts) ->
    Result = try id(Opts) of
        #{} -> decode(Payload);
        _ -> raw
    after
        put(seen, Payload)
    end,
    {Result, Payload, erase(seen)}.

try_later_clause(<<_, Payload/binary>>, Opts) ->
    Result = try id(Opts) of
        #{expect := Expected} when Expected =:= Payload -> expected;
        #{} -> decode(Payload);
        _ -> raw
    catch
        _:_ -> invalid
    end,
    {Result, Payload}.

try_inside_if(<<_, Payload/binary>>, Opts) ->
    Result = if
        is_map(Opts) ->
            try decode(Payload) of
                {text, _} -> Payload
            catch
                _:_ -> invalid
            end;
        true -> raw
    end,
    {Result, Payload}.

try_twice(<<_, Payload/binary>>, Opts) ->
    First = try id(Opts) of
        #{} -> decode(Payload);
        _ -> raw
    catch
        _:_ -> invalid
    end,
    Second = try id(Opts) of
        raw -> decode(Payload);
        _ -> skipped
    catch
        _:_ -> invalid
    end,
    {First, Second, Payload}.

try_then_compare(<<_, Payload/binary>>, Opts) ->
    Decoded = try id(Opts) of
        #{} -> decode(Payload);
        _ -> raw
    catch
        _:_ -> invalid
    end,
    Result = case id({<<"ping">>, Decoded}) of
        {Payload, _} -> same;
        _ -> different
    end,
    {Result, Payload}.

decode(Payload) -> {text, Payload}.

validate(Opts) when is_map(Opts) -> ok;
validate(_) -> error(badarg).

id(Value) -> Value.
