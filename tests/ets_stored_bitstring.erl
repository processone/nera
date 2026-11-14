%% EXPECT: ets_stored_bitstring_ok
%% Regression for non-byte-aligned bitstrings stored in ETS/persistent_term:
%% the writer's exit or garbage collection must not invalidate the value.
%% Copying reads must also survive deletion of the original storage.
%% Run this unchanged on native Nera and BEAM.
-module(ets_stored_bitstring).
-export([test/0]).

test() ->
    ets_lifetime(exit),
    ets_lifetime(gc),
    persistent_lifetime(exit),
    persistent_lifetime(gc),
    io:format("ets_stored_bitstring_ok~n"),
    ok.

ets_lifetime(Lifetime) ->
    T = ets:new(stored_bits, [set, public]),
    Writer = store(Lifetime, fun() ->
        true = ets:insert(T, {bits, values(), unchanged})
    end),
    churn(),
    [{bits, Actual, unchanged}] = ets:lookup(T, bits),
    equal({ets, Lifetime}, values(), Actual),
    %% Updating another element copies the bitstrings again.
    true = ets:update_element(T, bits, {3, [updated]}),
    [{bits, Listed, [updated]}] = ets:tab2list(T),
    true = ets:delete(T),
    churn(),
    equal({tab2list, Lifetime}, values(), Listed),
    stop_writer(Writer),
    ok.

persistent_lifetime(Lifetime) ->
    Key = {?MODULE, Lifetime},
    Writer = store(Lifetime, fun() ->
        ok = persistent_term:put(Key, values())
    end),
    churn(),
    Actual = persistent_term:get(Key),
    equal({persistent_term, Lifetime}, values(), Actual),
    true = persistent_term:erase(Key),
    churn(),
    equal({erased, Lifetime}, values(), Actual),
    stop_writer(Writer),
    ok.

%% Construct inside the writer, not in a closure captured from the reader.
%% Heap and refc binaries, sub-byte values, and tails starting inside a byte
%% all reach the shared storage copier as materialized bitstrings.
values() ->
    Small = <<"abcdefgh", (id(5)):4>>,
    Bytes = list_to_binary(lists:duplicate(id(200), $x)),
    Large = <<Bytes/binary, (id(3)):2>>,
    Boundary = list_to_binary(lists:duplicate(id(64), $b)),
    [{small, Small}, {large, Large}, {tiny, <<(id(5)):3>>},
     {boundary, <<Boundary/binary, (id(1)):1>>},
     {small_tail, tail(<<0:3, Small/bitstring>>, 3)},
     {large_tail, tail(<<"prefix", Large/bitstring>>, 48)}].

id(X) -> X.
tail(Bits, Skip) ->
    <<_:Skip, Rest/bitstring>> = Bits,
    Rest.

equal(Context, Expected, Actual) ->
    case Actual =:= Expected of
        true -> ok;
        false -> erlang:error({stored_bitstring_changed, Context, Expected, Actual})
    end.

store(exit, Body) ->
    {Pid, Ref} = spawn_monitor(fun() -> Body() end),
    down(Pid, Ref),
    none;
store(gc, Body) ->
    Parent = self(),
    {Pid, Ref} = spawn_monitor(fun() ->
        Body(),
        _ = [lists:seq(1, 64) || _ <- lists:seq(1, 2000)],
        true = erlang:garbage_collect(),
        _ = [lists:seq(1, 64) || _ <- lists:seq(1, 2000)],
        true = erlang:garbage_collect(),
        Parent ! {self(), stored},
        receive stop -> ok end
    end),
    receive
        {Pid, stored} -> {Pid, Ref};
        {'DOWN', Ref, process, Pid, Reason} -> erlang:error({writer_failed, Reason})
    after 5000 -> erlang:error(writer_timeout)
    end.

stop_writer(none) -> ok;
stop_writer({Pid, Ref}) ->
    Pid ! stop,
    down(Pid, Ref).

down(Pid, Ref) ->
    receive
        {'DOWN', Ref, process, Pid, normal} -> ok;
        {'DOWN', Ref, process, Pid, Reason} -> erlang:error({writer_failed, Reason})
    after 5000 -> erlang:error(writer_timeout)
    end.

%% Reuse freed writer/storage memory before reading it, with no timing sleep.
churn() ->
    Parent = self(),
    Workers = [spawn_monitor(fun() ->
        Held = [{I, lists:seq(1, 64)} || I <- lists:seq(1, 200)],
        Parent ! {self(), ready},
        receive stop -> length(Held) end
    end) || _ <- lists:seq(1, 32)],
    [receive {Pid, ready} -> ok after 5000 -> erlang:error(churn_timeout) end
     || {Pid, _} <- Workers],
    _ = [erlang:make_tuple(8, I) || I <- lists:seq(1, 20000)],
    true = erlang:garbage_collect(),
    [Pid ! stop || {Pid, _} <- Workers],
    [down(Pid, Ref) || {Pid, Ref} <- Workers],
    ok.
