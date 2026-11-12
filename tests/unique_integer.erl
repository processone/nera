-module(unique_integer).
-export([test/0]).
test() ->
    Parent = self(),
    lists:foreach(fun(_) -> spawn_link(fun() ->
        Values = [erlang:unique_integer() || _ <- lists:seq(1, 100)],
        Parent ! {values, Values}
    end) end, lists:seq(1, 4)),
    Values = gather(4, [erlang:unique_integer(), apply(erlang, unique_integer, [])]),
    true = lists:all(fun erlang:is_integer/1, Values),
    402 = length(lists:usort(Values)),
    ok.
gather(0, Acc) -> Acc;
gather(N, Acc) ->
    receive {values, Values} -> gather(N - 1, Values ++ Acc)
    after 5000 -> error(unique_integer_timeout) end.
