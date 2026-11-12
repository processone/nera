-module(receive_timeout_domain).
-export([test/0]).
test() ->
    lists:foreach(fun bad/1, [-1, invalid, [], 1.5, 4294967296, 1152921504606846976]),
    self() ! ready,
    ready = finite(4294967295),
    elapsed = finite(0),
    Parent = self(), spawn(fun() -> timer:sleep(10), Parent ! later end),
    later = finite(1000),
    %% A protected timeout failure must skip its receive and later expressions.
    caught = try receive _ -> consumed after invalid -> elapsed end,
                 error(continued_after_invalid)
             catch error:timeout_value -> caught end,
    {Pid, Ref} = spawn_monitor(fun() -> finite(invalid) end),
    receive {'DOWN', Ref, process, Pid, timeout_value} -> ok
    after 2000 -> error(uncaught_timeout_error_lost) end,
    ok.
finite(T) -> receive M -> M after T -> elapsed end.
bad(T) ->
    self() ! held,
    {'EXIT', {timeout_value, _}} = (catch receive held -> consumed after T -> elapsed end),
    held = receive M -> M after 0 -> missing end.
