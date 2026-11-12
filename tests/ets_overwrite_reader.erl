%% Tests that a process holding an ETS lookup result is unaffected
%% when another process overwrites the same key.
%% Phase 5 regression baseline: with zero-copy (Arc sharing), the old
%% Arc must remain valid for the reader even after overwrite drops it
%% from the table.
-module(ets_overwrite_reader).
-export([test/0]).

test() ->
    T = ets:new(overwrite_test, [set, public]),

    %% Insert original value
    OrigList = [1, 2, 3],
    ets:insert(T, {the_key, {data, OrigList}}),

    Parent = self(),

    %% Spawn reader that holds the lookup result
    spawn(fun() ->
        [{the_key, OldVal}] = ets:lookup(T, the_key),
        Parent ! {reader_got_old, self()},

        %% Wait for overwrite to happen
        receive go_verify -> ok end,

        %% Verify held result is still the OLD value (snapshot isolation)
        {data, [1, 2, 3]} = OldVal,

        %% New lookup should see new value
        [{the_key, NewVal}] = ets:lookup(T, the_key),
        {data, [4, 5, 6]} = NewVal,

        Parent ! {reader_done, self()}
    end),

    %% Wait for reader to have the old value
    ReaderPid = receive {reader_got_old, Pid} -> Pid end,

    %% Overwrite with new value
    ets:insert(T, {the_key, {data, [4, 5, 6]}}),

    %% Tell reader to verify
    ReaderPid ! go_verify,

    receive {reader_done, ReaderPid} -> ok end,

    ets:delete(T),
    ok.
