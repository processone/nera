%% EXPECT: max_heap_size_ok
%% cow_hpack requires the map form to enforce the same heap limit as the integer form.
-module(max_heap_size).
-export([test/0]).

test() ->
    Previous = process_flag(trap_exit, true),
    Limit = 1000,
    lists:foreach(fun(Value) ->
        Pid = spawn_link(fun() -> worker(Value) end),
        receive
            {'EXIT', Pid, killed} -> ok
        after 5000 ->
            error(timeout)
        end
    end, [Limit, #{size => Limit, kill => true}]),
    process_flag(trap_exit, Previous),
    io:format("max_heap_size_ok~n"),
    ok.

worker(Value) ->
    process_flag(max_heap_size, Value),
    %% Allocate until killed
    grow([]).

grow(Acc) ->
    grow([lists:seq(1, 100) | Acc]).
