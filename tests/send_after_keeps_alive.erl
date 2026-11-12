%% EXPECT: ok
-module(send_after_keeps_alive).
-export([test/0]).
test() ->
    erlang:send_after(50, self(), {delayed, <<"message">>}),
    receive {delayed, <<"message">>} -> ok end,
    io:format("ok~n").
