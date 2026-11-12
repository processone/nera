-module(spawn_probe).
-export([main/0]).
main() ->
    {Micros, ok} = timer:tc(fun() ->
        Parent = self(),
        [spawn(fun() -> producer(Parent, 10000) end) || _ <- lists:seq(1, 4)],
        collect(4, producer_done)
    end),
    io:format("spawn_real ~p~n", [Micros]).
producer(Parent, N) ->
    Me = self(),
    [spawn(fun() -> Me ! child_done end) || _ <- lists:seq(1, N)],
    collect(N, child_done),
    Parent ! producer_done.
collect(0, _) -> ok;
collect(N, Message) ->
    receive Message -> collect(N - 1, Message)
    after 30000 -> error(spawn_timeout) end.
