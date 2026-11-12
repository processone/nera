-module(hibernate_retained_roots).
-export([test/0, wake/2]).
%% EXPECT: hibernate_retained_roots ok

test() ->
    Parent = self(),
    Pid = spawn(fun() ->
        Key = {heap_key, lists:seq(1, 20)},
        Value = {lists:seq(1, 100), list_to_binary(lists:seq(0, 255)), 1152921504606846976},
        put(Key, Value),
        self() ! {retained, Value},
        erlang:hibernate(?MODULE, wake, [Parent, Key])
    end),
    %% No fresh message is sent: the queued message must wake the process.
    receive
        {retained_ok, Pid} -> io:format("hibernate_retained_roots ok~n")
    after 5000 -> error(hibernate_retained_roots_timeout)
    end.

wake(Parent, Key) ->
    Value = get(Key),
    receive
        {retained, Value = {List, Binary, BigInt}} ->
            List = lists:seq(1, 100),
            Binary = list_to_binary(lists:seq(0, 255)),
            1152921504606846976 = BigInt,
            Parent ! {retained_ok, self()}
    after 1000 -> error(retained_message_missing)
    end.
