%% Stress test: multiple processes insert diverse term types concurrently.
%% Phase 5 regression baseline: exercises all deep-copy paths under
%% concurrent access with diverse term types.
%% Each worker self-verifies its entries (insert + immediate lookup),
%% then a separate set of verifier processes re-check all entries
%% to exercise cross-process lookup after concurrent writes complete.
-module(ets_stress_complex).
-export([test/0]).

test() ->
    T = ets:new(stress, [set, public]),
    NumProcs = 10,
    EntriesPerProc = 200,
    Parent = self(),

    %% Phase 1: Spawn workers that insert diverse terms and self-verify
    lists:foreach(fun(P) ->
        spawn(fun() ->
            stress_loop(T, P, EntriesPerProc),
            Parent ! {done, self()}
        end)
    end, lists:seq(1, NumProcs)),

    wait_n(NumProcs),

    %% Phase 2: Spawn separate verifier processes (one per worker range)
    %% Each verifier runs in its own process to avoid heap accumulation
    lists:foreach(fun(P) ->
        spawn(fun() ->
            verify_range(T, P, EntriesPerProc),
            Parent ! {verified, P}
        end)
    end, lists:seq(1, NumProcs)),

    wait_verified(NumProcs),

    ets:delete(T),
    ok.

stress_loop(_, _, 0) -> ok;
stress_loop(T, P, N) ->
    Key = {P, N},
    Val = make_value(P, N),
    ets:insert(T, {Key, Val}),
    %% Immediately lookup and verify
    [{Key, Retrieved}] = ets:lookup(T, Key),
    verify_value(P, N, Retrieved),
    stress_loop(T, P, N - 1).

make_value(P, N) when N rem 5 =:= 0 ->
    %% List of integers
    lists:seq(P, P + 5);
make_value(_, N) when N rem 5 =:= 1 ->
    %% Nested tuple
    {nested, {level2, N}};
make_value(_, N) when N rem 5 =:= 2 ->
    %% Small binary
    list_to_binary(integer_to_list(N));
make_value(_, N) when N rem 5 =:= 3 ->
    %% Large binary (>64 bytes, triggers refc)
    list_to_binary(lists:duplicate(100, N rem 256));
make_value(_, N) ->
    %% Simple integer
    N * 100.

verify_value(P, N, Val) when N rem 5 =:= 0 ->
    Expected = lists:seq(P, P + 5),
    Expected = Val;
verify_value(_, N, Val) when N rem 5 =:= 1 ->
    {nested, {level2, N}} = Val;
verify_value(_, N, Val) when N rem 5 =:= 2 ->
    Expected = list_to_binary(integer_to_list(N)),
    Expected = Val;
verify_value(_, N, Val) when N rem 5 =:= 3 ->
    100 = byte_size(Val);
verify_value(_, N, Val) ->
    Expected = N * 100,
    Expected = Val.

verify_range(T, P, Count) ->
    verify_range(T, P, 1, Count).

verify_range(_, _, N, Max) when N > Max -> ok;
verify_range(T, P, N, Max) ->
    Key = {P, N},
    [{Key, Val}] = ets:lookup(T, Key),
    verify_value(P, N, Val),
    verify_range(T, P, N + 1, Max).

wait_n(0) -> ok;
wait_n(N) ->
    receive {done, _} -> wait_n(N - 1) end.

wait_verified(0) -> ok;
wait_verified(N) ->
    receive {verified, _} -> wait_verified(N - 1) end.
