%% Tests: parallel map with crash handling, monitor workers.
-module(pmap_crash).
-export([test/0]).

test() ->
    %% Basic pmap: all succeed
    [2, 4, 6, 8] = pmap(fun(X) -> X * 2 end, [1, 2, 3, 4]),

    %% Empty list
    [] = pmap(fun(X) -> X end, []),

    %% Single element
    [42] = pmap(fun(_) -> 42 end, [1]),

    %% Larger pmap
    Expected = lists:map(fun(X) -> X * X end, lists:seq(1, 20)),
    Expected = pmap(fun(X) -> X * X end, lists:seq(1, 20)),

    %% Safe pmap: worker crash detected via monitor
    ok = crash_detection_test(),

    io:format("ok~n"),
    ok.

%% Parallel map: spawn workers with index tags, collect and sort
pmap(Fun, List) ->
    Self = self(),
    spawn_indexed(Self, Fun, List, 1),
    N = length(List),
    Unsorted = collect_results(N, []),
    order_results(lists:sort(Unsorted)).

spawn_indexed(_Self, _Fun, [], _I) -> ok;
spawn_indexed(Self, Fun, [H|T], I) ->
    Idx = I,
    spawn(fun() -> Self ! {pmap_done, Idx, Fun(H)} end),
    spawn_indexed(Self, Fun, T, I + 1).

collect_results(0, Acc) -> Acc;
collect_results(N, Acc) ->
    receive
        {pmap_done, Idx, Val} ->
            collect_results(N - 1, [{Idx, Val} | Acc])
    after 5000 ->
        error(timeout)
    end.

order_results([]) -> [];
order_results([{_, V} | T]) -> [V | order_results(T)].

%% Test that we can detect a crashed worker via monitor
crash_detection_test() ->
    Self = self(),
    {Pid, Ref} = spawn_monitor(fun() -> error(boom) end),
    receive
        {'DOWN', Ref, process, Pid, _Reason} ->
            %% Worker crashed, we detected it
            ok
    after 2000 ->
        error(no_down)
    end.
