%% EXPECT: ok
-module(start_timer_keeps_alive).
-export([test/0]).
test() ->
    Ref = erlang:start_timer(50, self(), {delayed, <<"message">>}),
    receive {timeout, Ref, {delayed, <<"message">>}} -> ok end,
    io:format("ok~n").
