-module(bench_ets_updates).
-export([start/0]).

-define(N, 100000).

start() ->
    run(set),
    run(ordered_set),
    halt(0).

run(Kind) ->
    T = ets:new(bench, [Kind, public]),
    measure(Kind, insert, fun() -> insert(T, ?N) end),
    %% Retain original key storage so the pre-fix executable can be timed safely.
    Snapshot = snapshot(T, ?N, []),
    measure(Kind, overwrite, fun() -> insert(T, ?N) end),
    measure(Kind, immediate, fun() -> immediate(T, ?N) end),
    measure(Kind, boxed, fun() -> boxed(T, ?N) end),
    measure(Kind, lookup, fun() -> lookup(T, ?N) end),
    ?N = length(Snapshot),
    ets:delete(T),
    C = ets:new(concurrent, [Kind, public]),
    [ets:insert(C, {K, 0}) || K <- [1, 2, 3, 4]],
    Parent = self(),
    measure(Kind, concurrent, fun() ->
        [spawn(fun() -> updates(C, K, ?N), Parent ! done end) || K <- [1, 2, 3, 4]],
        [receive done -> ok end || _ <- [1, 2, 3, 4]]
    end),
    measure(Kind, concurrent_boxed, fun() ->
        [spawn(fun() -> boxed_updates(C, K, ?N div 4), Parent ! done end) || K <- [1, 2, 3, 4]],
        [receive done -> ok end || _ <- [1, 2, 3, 4]]
    end),
    ets:delete(C).

measure(Kind, Phase, F) ->
    T0 = erlang:monotonic_time(microsecond),
    F(),
    T1 = erlang:monotonic_time(microsecond),
    io:format("~p ~p ~p~n", [Kind, Phase, T1 - T0]).

insert(_, 0) -> ok;
insert(T, N) ->
    ets:insert(T, {{key, N}, {payload, N}, 0}),
    insert(T, N - 1).
immediate(_, 0) -> ok;
immediate(T, N) ->
    true = ets:update_element(T, {key, N}, {3, N}),
    immediate(T, N - 1).
boxed(_, 0) -> ok;
boxed(T, N) ->
    true = ets:update_element(T, {key, N}, {2, {updated, N}}),
    boxed(T, N - 1).
lookup(_, 0) -> ok;
lookup(T, N) ->
    [{{key, N}, {updated, N}, N}] = ets:lookup(T, {key, N}),
    lookup(T, N - 1).
updates(_, _, 0) -> ok;
updates(T, K, N) ->
    true = ets:update_element(T, K, {2, N}),
    updates(T, K, N - 1).

boxed_updates(_, _, 0) -> ok;
boxed_updates(T, K, N) ->
    true = ets:update_element(T, K, {2, {payload, N}}),
    boxed_updates(T, K, N - 1).

%% lookup retains the original backing owner; tab2list makes an independent copy.
snapshot(_, 0, Acc) -> Acc;
snapshot(T, N, Acc) ->
    snapshot(T, N - 1, [ets:lookup(T, {key, N}) | Acc]).
