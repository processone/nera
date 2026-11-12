-module(hibernate_gc_args).
-export([test/0, wake/2]).
%% EXPECT: hibernate_gc_args ok

test() ->
    Parent = self(),
    Pid = spawn(fun() ->
        %% One runtime allocation exceeds the young GC threshold immediately
        %% before hibernate; its pending MFA arguments must be owned before GC.
        LargeList = binary_to_list(binary:copy(<<7>>, 8192)),
        self() ! go,
        erlang:hibernate(?MODULE, wake, [Parent, LargeList])
    end),
    receive
        {gc_args_ok, Pid} -> io:format("hibernate_gc_args ok~n")
    after 5000 -> error(hibernate_gc_args_timeout)
    end.

wake(Parent, LargeList) ->
    receive go ->
        8192 = length(LargeList),
        7 = hd(LargeList),
        7 = lists:last(LargeList),
        Binary = binary:copy(<<7>>, 8192),
        Binary = list_to_binary(LargeList),
        Parent ! {gc_args_ok, self()}
    end.
