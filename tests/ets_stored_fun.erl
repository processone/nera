%% EXPECT: ets_stored_fun_ok
%% Regression: a fun stored in ETS or persistent_term keeps its captured
%% values after the process that stored it has exited or collected garbage,
%% as on BEAM. The storage copy used to keep only a pointer to the closure on
%% the storing process's heap, so a later call read freed memory: a wrong
%% result, a badfun or a crash. A fun read back with ets:tab2list/1 or
%% persistent_term:get/1 also kept pointing into the storage, so it broke
%% once the table or term was deleted.
%% Run this same fixture on native Nera and BEAM.
-module(ets_stored_fun).
-export([test/0]).

test() ->
    Expected = expected(),

    %% A process stores funs in a table, then exits.
    T = ets:new(stored_funs, [set, public]),
    in_process_that_exits(fun() ->
        F = make_fun(),
        true = ets:insert(T, {top, F}),
        true = ets:insert(T, {nested, [{wrapped, F}], #{f => F}, {F}}),
        true = ets:insert(T, {adder, make_adder(1000)}),
        true = ets:insert(T, {remote, fun lists:sum/1})
    end),
    churn(),
    [{top, Top}] = ets:lookup(T, top),
    Expected = Top(),
    [{nested, [{wrapped, InList}], #{f := InMap}, {InTuple}}] = ets:lookup(T, nested),
    Expected = InList(),
    Expected = InMap(),
    Expected = InTuple(),
    [{adder, Adder}] = ets:lookup(T, adder),
    1500500 = Adder(lists:seq(1, 1000)),
    [{remote, Remote}] = ets:lookup(T, remote),
    500500 = Remote(lists:seq(1, 1000)),

    %% Replacing another element copies the row again, fun included.
    true = ets:insert(T, {row, make_fun(), 1}),
    true = ets:update_element(T, row, {3, [updated]}),
    churn(),
    [{row, Updated, [updated]}] = ets:lookup(T, row),
    Expected = Updated(),

    %% A fun read with tab2list/1 keeps working after the table is deleted.
    Listed = [Fun || {top, Fun} <- ets:tab2list(T)],
    true = ets:delete(T),
    churn(),
    [Expected] = [Fun() || Fun <- Listed],

    %% The storing process stays alive but collects garbage.
    T2 = ets:new(stored_funs_gc, [set, public]),
    Self = self(),
    Holder = spawn(fun() ->
        true = ets:insert(T2, {f, make_fun()}),
        ok = collect_garbage(),
        Self ! {self(), stored},
        receive stop -> ok end
    end),
    receive {Holder, stored} -> ok end,
    churn(),
    [{f, AfterGc}] = ets:lookup(T2, f),
    Expected = AfterGc(),
    Holder ! stop,
    true = ets:delete(T2),

    %% persistent_term: stored by a process that exits, then read back.
    Key = {?MODULE, stored_fun},
    in_process_that_exits(fun() -> ok = persistent_term:put(Key, make_fun()) end),
    churn(),
    Stored = persistent_term:get(Key),
    Expected = Stored(),
    %% The fun read back stays whole after the term is erased.
    true = persistent_term:erase(Key),
    churn(),
    Expected = Stored(),

    io:format("ets_stored_fun_ok~n"),
    ok.

%% A fun capturing a list, small and large binaries, a tuple, a map with a
%% float, a bignum and another closure.
make_fun() ->
    Data = lists:seq(1, 1000),
    Small = <<"tag">>,
    Large = list_to_binary(lists:duplicate(200, $a)),
    Tuple = {point, [1, 2, 3]},
    Map = #{weight => 1.5, name => "fun"},
    Big = 1 bsl 100,
    Inner = fun(X) -> X + length(Data) end,
    fun() ->
        {lists:sum(Data), Small, byte_size(Large), Tuple, Map, Big, Inner(1)}
    end.

expected() ->
    {500500, <<"tag">>, 200, {point, [1, 2, 3]},
     #{weight => 1.5, name => "fun"}, 1 bsl 100, 1001}.

make_adder(N) ->
    fun(List) -> lists:sum(List) + N * length(List) end.

%% Run Body in a new process and return once that process has exited.
in_process_that_exits(Body) ->
    {Pid, Ref} = spawn_monitor(fun() -> Body() end),
    receive {'DOWN', Ref, process, Pid, normal} -> ok end.

%% Let other processes allocate and collect, so memory freed by an exited
%% process or a collection is used again.
churn() ->
    Self = self(),
    Pids = [spawn(fun() ->
                Held = [{I, lists:seq(1, 64)} || I <- lists:seq(1, 200)],
                Self ! {self(), ready},
                receive stop -> length(Held) end
            end) || _ <- lists:seq(1, 100)],
    [receive {Pid, ready} -> ok end || Pid <- Pids],
    _ = [erlang:make_tuple(8, I) || I <- lists:seq(1, 20000)],
    true = erlang:garbage_collect(),
    [Pid ! stop || Pid <- Pids],
    ok.

collect_garbage() ->
    _ = [lists:seq(1, 64) || _ <- lists:seq(1, 2000)],
    true = erlang:garbage_collect(),
    _ = [lists:seq(1, 64) || _ <- lists:seq(1, 2000)],
    true = erlang:garbage_collect(),
    ok.
