%% EXPECT: copy_deep_nesting_ok
%% Regression: a term nested 200,000 levels deep can be sent to another
%% process, captured in a spawned fun, and stored in and read back from ETS,
%% as a value or as a key, as on BEAM. The message copier and the ETS copies
%% used to recurse on the native stack for list heads and tuple and map
%% elements, so an iolist built by prepending ([Acc, Chunk]) crashed the
%% program with a bus error: in ETS from about 5,000 levels, when sent or
%% captured from about 14,000.
%% Run this same fixture on native Nera and BEAM.
-module(copy_deep_nesting).
-export([test/0]).

-define(DEPTH, 200000).

test() ->
    Terms = [{iolist, iolist(?DEPTH, [])},
             {tuple, tuple(?DEPTH, done)},
             {map, map(?DEPTH, done)},
             {closure, closure(?DEPTH, done)}],

    %% Send each term to another process, which checks its copy and sends
    %% that copy back.
    Echo = spawn(fun echo/0),
    [ok = round_trip(Echo, Term) || Term <- Terms],

    %% Capture each term in a spawned fun.
    [ok = spawn_capture(Term) || Term <- Terms],

    %% Store the iolist, tuple and map in ETS and read them back by key,
    %% as the whole table, and from another process.
    Stored = [Term || {Kind, _} = Term <- Terms, Kind =/= closure],
    Table = ets:new(copy_deep_nesting, [set, public]),
    true = ets:insert(Table, Stored),
    Found = [Entry || {Kind, _} <- Stored, Entry <- ets:lookup(Table, Kind)],
    3 = length(Found),
    [ok = check(Entry) || Entry <- Found],
    [ok = round_trip(Echo, Entry) || Entry <- Found],
    3 = length([ok = check(Entry) || Entry <- ets:tab2list(Table)]),
    Self = self(),
    spawn(fun() -> Self ! {reader, [checked(Entry) || Entry <- ets:tab2list(Table)]} end),
    receive {reader, Results} -> [ok, ok, ok] = Results end,

    %% A term this deep can be a key too, found again with an equal term
    %% built separately.
    {iolist, IoList} = lists:keyfind(iolist, 1, Terms),
    true = ets:insert(Table, {IoList, deep_key}),
    [{Key, deep_key}] = ets:lookup(Table, iolist(?DEPTH, [])),
    ok = check({iolist, Key}),

    Echo ! stop,
    io:format("copy_deep_nesting_ok~n"),
    ok.

echo() ->
    receive
        {From, Term} ->
            From ! {echoed, checked(Term), Term},
            echo();
        stop ->
            ok
    end.

round_trip(Echo, Term) ->
    Echo ! {self(), Term},
    receive {echoed, Result, Back} -> ok = Result, check(Back) end.

spawn_capture(Term) ->
    Self = self(),
    Pid = spawn(fun() -> Self ! {captured, self(), checked(Term)} end),
    receive {captured, Pid, Result} -> Result end.

%% The check's result in another process, which reports a failure instead
%% of dying silently.
checked(Term) ->
    try check(Term) catch Class:Reason -> {Class, Reason} end.

check({iolist, IoList}) -> ?DEPTH = iolist_depth(IoList, 0), ok;
check({tuple, Tuple}) -> ?DEPTH = tuple_depth(Tuple, 0), ok;
check({map, Map}) -> ?DEPTH = map_depth(Map, 0), ok;
check({closure, Fun}) -> ?DEPTH = closure_depth(Fun, 0), ok.

iolist(0, Acc) -> Acc;
iolist(N, Acc) -> iolist(N - 1, [Acc, <<"chunk">>]).

iolist_depth([], Depth) -> Depth;
iolist_depth([Inner, <<"chunk">>], Depth) -> iolist_depth(Inner, Depth + 1).

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
