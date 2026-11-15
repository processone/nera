%% EXPECT: ets_heir_ok
%% The configured live heir inherits data and ownership before EXIT/DOWN.
%% Gift data must own its storage across the creator's GC and exit, as on BEAM.
-module(ets_heir).
-export([test/0]).

test() ->
    process_flag(trap_exit, true),
    lists:foreach(fun(Type) ->
        lists:foreach(fun(Named) ->
            lists:foreach(fun(Mode) -> inherit(Type, Named, Mode) end,
                          [normal, reason, killed, cascade])
        end, [false, true])
    end, [set, ordered_set, bag, duplicate_bag]),
    no_heir(none),
    no_heir(dead_before_creation),
    no_heir(dead_after_creation),
    no_heir(self),
    explicit_delete(),
    legacy_heir(),
    give_away_keeps_heir(),
    heir_exit_cleanup(),
    options(),
    io:format("ets_heir_ok~n"),
    ok.

inherit(Type, Named, Mode) ->
    Parent = self(),
    Ref = make_ref(),
    Leader = spawn(fun() -> receive die -> exit(cascade_reason) end end),
    Owner = spawn_link(fun() ->
        case Mode of cascade -> link(Leader); _ -> ok end,
        T = create(Type, Named, Parent, Ref),
        true = erlang:garbage_collect(),
        Parent ! {ready, self(), T},
        receive
            die -> case Mode of normal -> ok; _ -> exit(owner_reason) end
        end
    end),
    T = receive {ready, Owner, Tab} -> Tab after 5000 -> exit(owner_not_ready) end,
    Monitor = monitor(process, Owner),
    case Mode of
        killed -> exit(Owner, kill);
        cascade -> Leader ! die;
        _ -> Owner ! die
    end,
    %% Read the very next message, not a selective receive skipping EXIT/DOWN.
    Gift = receive
        Message -> {'ETS-TRANSFER', T, Owner, ReceivedGift} = Message,
                   ok = check_gift(ReceivedGift, Ref),
                   ReceivedGift
    after 5000 -> exit(missing_transfer)
    end,
    Parent = ets:info(T, owner),
    Parent = ets:info(T, heir),
    [{row, Type, Ref}] = ets:lookup(T, row),
    true = ets:insert(T, {new_owner, Parent}),
    [{new_owner, Parent}] = ets:lookup(T, new_owner),
    io:format("heir_transfer type=~p named=~p exit=~p table=~p from=~p owner=~p rows=~p~n",
              [Type, Named, Mode, T, Owner, ets:info(T, owner), ets:tab2list(T)]),
    receive {'EXIT', Owner, _} -> ok after 5000 -> exit(missing_exit) end,
    receive {'DOWN', Monitor, process, Owner, _} -> ok after 5000 -> exit(missing_down) end,
    true = ets:delete(T),
    ok = check_gift(Gift, Ref),
    %% Do not leave the unused cascade leader alive.
    Stop = monitor(process, Leader),
    Leader ! die,
    receive {'DOWN', Stop, process, Leader, _} -> ok end.

create(Type, Named, Heir, Ref) ->
    Base = [Type, {heir, Heir, make_gift(Ref)}],
    Opts = case Named of true -> [named_table | Base]; false -> Base end,
    T = ets:new(heir_table, Opts),
    true = ets:insert(T, {row, Type, Ref}),
    T.

make_gift(Ref) ->
    List = lists:seq(1, 200),
    Binary = list_to_binary(lists:duplicate(200, 173)),
    <<_:3, Bits:1593/bitstring, _:4>> = Binary,
    Fun = fun() -> {Ref, lists:sum(List), Bits} end,
    {gift, Ref, #{list => List, bits => Bits, fun_value => Fun}, 1 bsl 100}.

check_gift({gift, Ref, #{list := List, bits := Bits, fun_value := Fun}, Big}, Ref) ->
    List = lists:seq(1, 200),
    1593 = bit_size(Bits),
    Binary = list_to_binary(lists:duplicate(200, 173)),
    <<_:3, Bits:1593/bitstring, _:4>> = Binary,
    {Ref, 20100, Bits} = Fun(),
    Big = 1 bsl 100,
    ok.

no_heir(Mode) ->
    Parent = self(),
    Heir = spawn(fun() -> receive stop -> ok end end),
    HMon = monitor(process, Heir),
    case Mode of
        dead_before_creation -> Heir ! stop, receive {'DOWN', HMon, process, Heir, _} -> ok end;
        _ -> ok
    end,
    Owner = spawn(fun() ->
        H = case Mode of self -> self(); _ -> Heir end,
        Opt = case Mode of none -> {heir, none}; _ -> {heir, H, ignored} end,
        T = ets:new(no_heir_table, [named_table, Opt]),
        Parent ! {ready, self(), T},
        receive stop -> ok end
    end),
    T = receive {ready, Owner, Tab} -> Tab end,
    Mon = monitor(process, Owner),
    case Mode of dead_before_creation -> none = ets:info(T, heir); _ -> ok end,
    case Mode of
        dead_after_creation -> Heir ! stop, receive {'DOWN', HMon, process, Heir, _} -> ok end;
        _ -> ok
    end,
    Owner ! stop,
    receive {'DOWN', Mon, process, Owner, normal} -> ok end,
    undefined = ets:info(T, owner),
    T = ets:new(no_heir_table, [named_table]),
    true = ets:delete(T),
    case Mode of
        dead_before_creation -> ok;
        dead_after_creation -> ok;
        _ -> Heir ! stop, receive {'DOWN', HMon, process, Heir, _} -> ok end
    end.

explicit_delete() ->
    Parent = self(),
    {Owner, Mon} = spawn_monitor(fun() ->
        T = ets:new(deleted_heir_table, [{heir, Parent, deleted}]),
        true = ets:delete(T)
    end),
    receive {'DOWN', Mon, process, Owner, normal} -> ok end,
    receive {'ETS-TRANSFER', _, Owner, _} -> exit(transfer_after_delete) after 0 -> ok end.

%% OTP also accepts the legacy form without gift data or notification.
legacy_heir() ->
    Parent = self(),
    {Owner, Mon} = spawn_monitor(fun() ->
        T = ets:new(legacy_heir_table, [{heir, Parent}]),
        true = ets:insert(T, {key, legacy}),
        Parent ! {legacy_table, T}
    end),
    T = receive {legacy_table, Tab} -> Tab end,
    receive {'DOWN', Mon, process, Owner, normal} -> ok end,
    Parent = ets:info(T, owner),
    [{key, legacy}] = ets:lookup(T, key),
    receive {'ETS-TRANSFER', _, Owner, _} -> exit(legacy_notification) after 0 -> ok end,
    true = ets:delete(T).

%% A table still uses its configured heir after a manual ownership transfer.
give_away_keeps_heir() ->
    Parent = self(),
    Next = spawn(fun() ->
        receive {'ETS-TRANSFER', T, Parent, manual} ->
            Parent ! {received, self(), T},
            receive stop -> ok end
        end
    end),
    T = ets:new(given_heir_table, [{heir, Parent, inherited}]),
    true = ets:insert(T, {key, preserved}),
    true = ets:give_away(T, Next, manual),
    receive {received, Next, T} -> ok end,
    Mon = monitor(process, Next),
    Next ! stop,
    receive {'ETS-TRANSFER', T, Next, inherited} -> ok after 5000 -> exit(missing_inherited_transfer) end,
    Parent = ets:info(T, owner),
    [{key, preserved}] = ets:lookup(T, key),
    receive {'DOWN', Mon, process, Next, normal} -> ok end,
    true = ets:delete(T).

%% The ownership index must include inherited tables, including when an heir
%% exits at the same time as the creator (either deletion or inheritance wins).
heir_exit_cleanup() ->
    lists:foreach(fun(_) ->
        Parent = self(),
        Heir = spawn(fun() -> receive stop -> ok end end),
        HM = monitor(process, Heir),
        {Owner, OM} = spawn_monitor(fun() ->
            T = ets:new(heir_cleanup_table, [named_table, {heir, Heir, cleanup}]),
            Parent ! {ready, self(), T},
            receive stop -> ok end
        end),
        T = receive {ready, Owner, Tab} -> Tab end,
        Owner ! stop,
        Heir ! stop,
        receive {'DOWN', OM, process, Owner, normal} -> ok end,
        receive {'DOWN', HM, process, Heir, normal} -> ok end,
        undefined = ets:info(T, owner),
        T = ets:new(heir_cleanup_table, [named_table]),
        true = ets:delete(T)
    end, lists:seq(1, 100)).

options() ->
    Self = self(),
    T = ets:new(heir_options, [{heir, Self, first}, {heir, none}]),
    none = ets:info(T, heir),
    true = ets:delete(T),
    T2 = ets:new(heir_options, [{heir, none}, {heir, Self, last}]),
    Self = ets:info(T2, heir),
    true = ets:delete(T2),
    lists:foreach(fun(Opt) ->
        try ets:new(heir_options, [Opt]) of
            _ -> exit({accepted_invalid_heir, Opt})
        catch error:badarg -> ok end
    end, [heir, {heir, 42, gift}, {heir, none, gift}, {heir},
          {heir, Self, gift, extra}]).
