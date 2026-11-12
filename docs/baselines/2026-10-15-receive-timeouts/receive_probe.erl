-module(receive_probe).
-export([main/0]).
main() ->
    bench(plain, plain, 100000), bench(after_zero, 0, 100000),
    bench(after_finite, 1000, 100000), bench(after_infinity, infinity, 100000),
    poll(100), {Time, ok} = timer:tc(fun() -> poll(10000) end),
    io:format("empty_poll ~p~n", [Time]).
bench(Name, Timeout, Count) ->
    loop(Timeout, 100),
    {Time, ok} = timer:tc(fun() -> loop(Timeout, Count) end),
    io:format("~p ~p~n", [Name, Time]).
loop(_, 0) -> ok;
loop(plain, N) -> self() ! ready, receive ready -> ok end, loop(plain, N - 1);
loop(T, N) -> self() ! ready,
    receive ready -> ok after T -> error(unexpected_timeout) end, loop(T, N - 1).
poll(0) -> ok;
poll(N) -> receive never -> error(unexpected_message) after 0 -> ok end, poll(N - 1).
