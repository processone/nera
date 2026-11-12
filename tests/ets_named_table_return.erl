%% EXPECT: ets_named_table_return_ok
%% Regression: ets:new(Name, [named_table | _]) returned a number natively
%% instead of Name, so the common `?MODULE = ets:new(?MODULE, [named_table,
%% ...])` match failed with badmatch, as in Cowboy's cowboy_clock:init/1.
%% The returned name must keep naming that table: a table created without
%% named_table does not take the name, and creating or deleting one with the
%% same atom leaves the named table reachable. Release of the name when the
%% owner exits is checked by the runtime's unit tests, because natively the
%% monitor message can arrive before the table is deleted. Run this fixture on
%% native Nera and BEAM.
-module(ets_named_table_return).
-export([test/0]).

-define(TAB, ets_named_table_return).

test() ->
    %% The idiom from cowboy_clock:init/1.
    ?TAB = ets:new(?TAB, [set, protected, named_table, {read_concurrency, true}]),
    true = ets:insert(?TAB, {rfc1123, <<"Thu, 12 Nov 2026 00:00:00 GMT">>}),
    [{rfc1123, _}] = ets:lookup(?TAB, rfc1123),
    ?TAB = ets:info(?TAB, name),
    true = ets:delete(?TAB),
    undefined = ets:info(?TAB),

    %% Every table type, with the name bound at run time.
    lists:foreach(fun(Type) ->
        Name = id(list_to_atom("ets_named_" ++ atom_to_list(Type))),
        Name = ets:new(Name, [Type, public, named_table]),
        true = ets:insert(Name, {k, Type}),
        [{k, Type}] = ets:lookup(Name, k),
        Type = ets:info(Name, type),
        true = ets:delete(Name)
    end, [set, ordered_set, bag, duplicate_bag]),

    %% named_table anywhere in the options list.
    ets_named_last = ets:new(ets_named_last, [public, {keypos, 2}, named_table]),
    2 = ets:info(ets_named_last, keypos),
    true = ets:delete(ets_named_last),

    %% Apply, a variable module and fun references.
    ets_named_apply = apply(ets, new, [ets_named_apply, [named_table]]),
    Mod = id(ets),
    ets_named_var = Mod:new(ets_named_var, [named_table]),
    ets_named_fun = (fun ets:new/2)(id(ets_named_fun), [named_table]),
    New = id(new),
    ets_named_dyn = (fun Mod:New/2)(ets_named_dyn, [named_table]),
    lists:foreach(fun(T) -> true = ets:delete(T) end,
                  [ets_named_apply, ets_named_var, ets_named_fun, ets_named_dyn]),

    %% Without named_table the result is not the name and does not take it.
    Unnamed = ets:new(ets_named_shared, [public]),
    false = is_atom(Unnamed),
    true = ets:insert(Unnamed, {k, unnamed}),
    [{k, unnamed}] = ets:lookup(Unnamed, k),
    undefined = ets:info(ets_named_shared),

    %% A named table may then take the atom, and the name reaches it.
    ets_named_shared = ets:new(ets_named_shared, [public, named_table]),
    true = ets:insert(ets_named_shared, {k, named}),
    [{k, named}] = ets:lookup(ets_named_shared, k),
    [{k, unnamed}] = ets:lookup(Unnamed, k),

    %% An unnamed table created later with the atom leaves the name alone.
    Later = ets:new(ets_named_shared, [public]),
    true = ets:insert(Later, {k, later}),
    [{k, named}] = ets:lookup(ets_named_shared, k),

    %% Deleting an unnamed table keeps the named table reachable.
    true = ets:delete(Unnamed),
    true = ets:delete(Later),
    [{k, named}] = ets:lookup(ets_named_shared, k),
    true = ets:delete(ets_named_shared),
    undefined = ets:info(ets_named_shared),

    %% A deleted name can be used again.
    ets_named_shared = ets:new(ets_named_shared, [named_table]),
    true = ets:delete(ets_named_shared),

    %% A named table created by another process, as a gen_server init does,
    %% is reached by its name.
    Parent = self(),
    {Pid, Ref} = spawn_monitor(fun() ->
        Parent ! {created, self(), ets:new(ets_named_owner, [public, named_table])},
        receive stop -> ok end
    end),
    Created = receive
        {created, Pid, C} -> C;
        {'DOWN', Ref, process, Pid, Reason} -> error({creator_exited, Reason})
    end,
    ets_named_owner = Created,
    true = ets:insert(ets_named_owner, {k, owner}),
    [{k, owner}] = ets:lookup(ets_named_owner, k),
    Pid ! stop,
    receive {'DOWN', Ref, process, Pid, normal} -> ok end,

    io:format("ets_named_table_return_ok~n"),
    ok.

id(X) -> X.
