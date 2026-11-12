%% EXPECT: map_iteration_order_ok
%% Regression (Cowlib on Nera): on BEAM a map of up to 32 keys iterates in
%% ascending key order (atoms by atom-table index, everything else by term
%% order). Nera iterated in insertion order, so Cowlib's setcookie attributes,
%% HTTP/2 pseudo-headers and SETTINGS frames came out in another order.
%% Atom index order is a VM-internal detail and not asserted here: atom keys
%% are only checked for agreement between maps:keys/values/to_list/fold/
%% iterator and ~w. Maps of 33+ keys iterate in hash order on BEAM and only
%% their entry set is asserted. Run this fixture on native Nera and BEAM.
-module(map_iteration_order).
-export([test/0]).

test() ->
    %% Non-atom keys have a fixed order, whatever the insertion order.
    M = id(#{<<"b">> => 1, {t} => 2, 2.0 => 3, 1 => 5, [] => 6, "x" => 7}),
    [1, 2.0, {t}, [], "x", <<"b">>] = maps:keys(M),
    [5, 3, 2, 6, 7, 1] = maps:values(M),
    P = lists:foldl(fun(I, Acc) -> Acc#{I => I} end, #{}, lists:seq(32, 1, -1)),
    true = maps:keys(P) =:= lists:seq(1, 32),
    B = id(#{<<"set-cookie">> => 1, <<"accept">> => 2, <<"host">> => 3}),
    [<<"accept">>, <<"host">>, <<"set-cookie">>] = maps:keys(B),
    %% Every view of one atom-keyed map agrees on a single order.
    A = id(#{path => 3, max_age => 1, domain => 2, secure => 4}),
    Pairs = maps:to_list(A),
    true = maps:keys(A) =:= [K || {K, _} <- Pairs],
    true = maps:values(A) =:= [V || {_, V} <- Pairs],
    true = lists:reverse(maps:fold(fun(K, V, Acc) -> [{K, V} | Acc] end, [], A)) =:= Pairs,
    true = drain(maps:iterator(A)) =:= Pairs,
    Printed = lists:flatten(io_lib:format("~w", [A])),
    Printed = lists:flatten(["#{", lists:join(",", [io_lib:format("~w => ~w", [K, V]) || {K, V} <- Pairs]), "}"]),
    %% Beyond 32 keys the set of entries is the contract.
    L = maps:from_list([{I, I} || I <- lists:seq(40, 1, -1)]),
    true = lists:sort(maps:keys(L)) =:= lists:seq(1, 40),
    io:format("map_iteration_order_ok~n"),
    ok.

drain(It) -> drain(maps:next(It), []).
drain(none, Acc) -> lists:reverse(Acc);
drain({K, V, Next}, Acc) -> drain(maps:next(Next), [{K, V} | Acc]).

id(X) -> X.
