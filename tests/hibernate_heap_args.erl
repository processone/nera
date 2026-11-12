-module(hibernate_heap_args).
-export([test/0, wake/2]).
%% EXPECT: hibernate_heap_args ok
test() ->
    Parent = self(),
    Pid = spawn(fun() ->
        Value = {lists:seq(1, 100), <<1,2,3,4>>, 1152921504606846976},
        erlang:hibernate(?MODULE, wake, [Parent, Value])
    end),
    timer:sleep(50),
    Pid ! go,
    receive
        {result, {List, <<1,2,3,4>>, 1152921504606846976}} ->
            List = lists:seq(1, 100),
            io:format("hibernate_heap_args ok~n")
    after 5000 -> error(timeout)
    end.
wake(Parent, Value) ->
    receive go -> Parent ! {result, Value} end.
