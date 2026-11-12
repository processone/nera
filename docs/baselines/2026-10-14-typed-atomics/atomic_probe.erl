-module(atomic_probe).
-export([main1/0, main2/0, main4/0, main8/0]).
main1() -> run(1).
main2() -> run(2).
main4() -> run(4).
main8() -> run(8).
run(Threads) ->
    phase(read, Threads), phase(add, Threads), phase(add_get, Threads), phase(cas, Threads).
phase(Kind, Threads) ->
    A = atomics:new(1, []), Parent = self(),
    {Micros, ok} = timer:tc(fun() ->
        [spawn(fun() -> loop(Kind, A, 320000 div Threads), Parent ! done end)
         || _ <- lists:seq(1, Threads)],
        collect(Threads)
    end),
    Expected = case Kind of read -> 0; cas -> 0; _ -> 320000 end,
    Expected = atomics:get(A, 1),
    io:format("~p ~p~n", [Kind, Micros]).
loop(_, _, 0) -> ok;
loop(read, A, N) -> 0 = atomics:get(A, 1), loop(read, A, N - 1);
loop(add, A, N) -> ok = atomics:add(A, 1, 1), loop(add, A, N - 1);
loop(add_get, A, N) -> atomics:add_get(A, 1, 1), loop(add_get, A, N - 1);
loop(cas, A, N) -> ok = atomics:compare_exchange(A, 1, 0, 0), loop(cas, A, N - 1).
collect(0) -> ok;
collect(N) -> receive done -> collect(N - 1) after 30000 -> error(probe_timeout) end.
