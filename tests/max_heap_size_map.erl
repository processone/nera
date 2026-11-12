%% EXPECT: max_heap_size_map_ok
%% cow_hpack's oversized-integer test sets a default-policy max_heap_size map.
-module(max_heap_size_map).
-export([test/0]).

test() ->
    Initial = process_flag(max_heap_size, #{size => 100000, kill => true}),
    #{size := _, kill := true, error_logger := true, include_shared_binaries := false} = Initial,
    #{size := 100000, kill := true} = process_flag(max_heap_size, #{size => 200000}),
    #{size := 200000} = process_flag(max_heap_size, 300000),
    #{size := 300000} = apply(erlang, process_flag, [max_heap_size, #{size => 400000}]),
    F = fun erlang:process_flag/2,
    #{size := 400000} = F(max_heap_size, #{size => 500000, error_logger => true}),
    #{size := 500000, error_logger := true} = F(max_heap_size, Initial),
    lists:foreach(fun(Value) ->
        badarg = reason(fun() -> process_flag(max_heap_size, Value) end)
    end, [#{}, #{kill => true}, #{size => -1}, #{size => 1.0},
          #{size => 100000, kill => invalid}, #{size => 100000, error_logger => invalid}]),
    io:format("max_heap_size_map_ok~n"),
    ok.
reason(F) -> try F() of Value -> {unexpected, Value} catch error:Reason -> Reason end.
