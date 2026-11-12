%% Tests that ETS lookup results can be sent as messages between processes.
%% Phase 5 regression baseline: with zero-copy, external pointers must
%% be properly deep-copied (strip external bit) when sent as messages.
-module(ets_message_passing).
-export([test/0]).

test() ->
    T = ets:new(msg_test, [set, public]),

    %% Insert complex terms
    ets:insert(T, {key1, [10, 20, 30]}),
    ets:insert(T, {key2, {nested, {inner, 42}}}),
    ets:insert(T, {key3, list_to_binary(lists:duplicate(100, 66))}),

    Parent = self(),

    %% Spawn a process that looks up and sends results back
    spawn(fun() ->
        [{key1, L}] = ets:lookup(T, key1),
        [{key2, N}] = ets:lookup(T, key2),
        [{key3, B}] = ets:lookup(T, key3),
        Parent ! {results, L, N, B}
    end),

    receive
        {results, List, Nested, Bin} ->
            [10, 20, 30] = List,
            {nested, {inner, 42}} = Nested,
            100 = byte_size(Bin)
    end,

    %% Also test: lookup in one process, send to another directly
    [{key1, MyList}] = ets:lookup(T, key1),
    spawn(fun() ->
        Parent ! {forwarded, MyList}
    end),

    %% The above sends MyList (which came from ETS lookup) via closure env.
    %% With zero-copy, MyList contains external pointers that must be
    %% deep-copied when the closure is sent to the new process.
    receive
        {forwarded, FL} ->
            [10, 20, 30] = FL
    end,

    ets:delete(T),
    ok.
