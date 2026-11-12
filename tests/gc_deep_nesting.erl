%% EXPECT: gc_deep_nesting_ok
%% Regression: a process holding a term nested 200,000 levels deep survives
%% garbage collection, as on BEAM. The collector used to copy list heads and
%% tuple, map and closure elements recursively on the native stack, so an
%% iolist built by prepending ([Acc, Chunk]) crashed the program with a bus
%% error once a collection ran (from about 13,000 levels). Collections run
%% here both on request and from allocation while the terms are alive.
%% Run this same fixture on native Nera and BEAM.
-module(gc_deep_nesting).
-export([test/0]).

-define(DEPTH, 200000).

test() ->
    %% An iolist built by prepending nests each level in head position.
    IoList = iolist(?DEPTH, []),
    true = erlang:garbage_collect(),
    ok = churn(50000, IoList),
    ?DEPTH = iolist_depth(IoList, 0),

    %% The same nesting in tuples, maps and closure environments.
    Tuple = tuple(?DEPTH, done),
    Map = map(?DEPTH, done),
    Fun = closure(?DEPTH, done),
    true = erlang:garbage_collect(),
    ?DEPTH = tuple_depth(Tuple, 0),
    ?DEPTH = map_depth(Map, 0),
    ?DEPTH = closure_depth(Fun, 0),

    io:format("gc_deep_nesting_ok~n"),
    ok.

iolist(0, Acc) -> Acc;
iolist(N, Acc) -> iolist(N - 1, [Acc, <<"chunk">>]).

iolist_depth([], Depth) -> Depth;
iolist_depth([Inner, <<"chunk">>], Depth) -> iolist_depth(Inner, Depth + 1).

%% Allocate short-lived data so collections run while IoList is alive, and
%% check after each round that its outermost levels are intact.
churn(0, _IoList) -> ok;
churn(N, IoList) ->
    20 = length([{N, <<N:64>>} || _ <- lists:seq(1, 20)]),
    [[_, <<"chunk">>], <<"chunk">>] = IoList,
    churn(N - 1, IoList).

tuple(0, Acc) -> Acc;
tuple(N, Acc) -> tuple(N - 1, {Acc, N}).

tuple_depth(done, Depth) -> Depth;
tuple_depth({Inner, N}, Depth) when N =:= Depth + 1 -> tuple_depth(Inner, N).

map(0, Acc) -> Acc;
map(N, Acc) -> map(N - 1, #{inner => Acc, level => N}).

map_depth(done, Depth) -> Depth;
map_depth(#{inner := Inner, level := N}, Depth) when N =:= Depth + 1 ->
    map_depth(Inner, N).

closure(0, Acc) -> Acc;
closure(N, Acc) -> closure(N - 1, fun() -> {Acc, N} end).

closure_depth(done, Depth) -> Depth;
closure_depth(Fun, Depth) ->
    {Inner, N} = Fun(),
    N = Depth + 1,
    closure_depth(Inner, N).
