-module(process_info_snapshot_roots).
-export([test/0]).
%% EXPECT: process_info_snapshot_roots ok

test() ->
    Parent = self(),
    Pid = spawn(fun() ->
        Key = {heap_key, [1,2,3]},
        Value = {lists:seq(1, 100), list_to_binary(lists:seq(0, 255))},
        put(Key, Value),
        self() ! {payload, Value},
        Parent ! {ready, self()},
        receive stop -> ok end
    end),
    Ref = monitor(process, Pid),
    receive {ready, Pid} -> ok after 5000 -> error(ready_timeout) end,
    Snapshot = process_info(Pid, [messages, dictionary]),
    Pid ! stop,
    receive {'DOWN', Ref, process, Pid, normal} -> ok
    after 5000 -> error(exit_timeout) end,
    %% The queried process and its heap have now been released.
    [{messages, [{payload, FromMessage}]},
     {dictionary, [{{heap_key, [1,2,3]}, FromDictionary}]}] = Snapshot,
    FromMessage = FromDictionary,
    {List, Binary} = FromMessage,
    List = lists:seq(1, 100),
    Binary = list_to_binary(lists:seq(0, 255)),
    io:format("process_info_snapshot_roots ok~n"),
    ok.
