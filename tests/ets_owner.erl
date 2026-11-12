%% Tests for ETS table ownership and cleanup on process death.
-module(ets_owner).
-export([test/0]).

test() ->
    test_owner_is_self(),
    test_table_deleted_on_owner_death(),
    test_multiple_tables_cleaned_up(),
    test_give_away_transfers_ownership(),
    test_give_away_owner_death(),
    test_linked_cascade_deletes_table(),
    test_non_owner_death_no_effect(),
    test_owner_death_concurrent_access(),
    ok.

%% ---------------------------------------------------------------------------
%% Test 1: ets:info(Tab, owner) returns self() for the creating process.
%% ---------------------------------------------------------------------------
test_owner_is_self() ->
    Tab = ets:new(owner_test_1, [set, public]),
    Self = self(),
    Self = ets:info(Tab, owner),
    ets:delete(Tab),
    ok.

%% ---------------------------------------------------------------------------
%% Test 2: Table is automatically deleted when the owning process exits.
%% ---------------------------------------------------------------------------
test_table_deleted_on_owner_death() ->
    Parent = self(),
    spawn(fun() ->
        Tab = ets:new(owner_test_2, [set, public, named_table]),
        ets:insert(Tab, {key, value}),
        Parent ! {table, Tab},
        %% Process exits normally here
        ok
    end),
    Tab = receive {table, T} -> T end,
    %% Give the spawned process time to exit and be cleaned up
    timer:sleep(50),
    %% Table should be gone — ets:info returns undefined for deleted tables
    undefined = ets:info(Tab, size),
    ok.

%% ---------------------------------------------------------------------------
%% Test 3: Multiple tables owned by one process are all cleaned up.
%% ---------------------------------------------------------------------------
test_multiple_tables_cleaned_up() ->
    Parent = self(),
    spawn(fun() ->
        T1 = ets:new(multi_1, [set, public]),
        T2 = ets:new(multi_2, [set, public]),
        T3 = ets:new(multi_3, [set, public]),
        ets:insert(T1, {a, 1}),
        ets:insert(T2, {b, 2}),
        ets:insert(T3, {c, 3}),
        Parent ! {tables, T1, T2, T3},
        ok
    end),
    {T1, T2, T3} = receive {tables, A, B, C} -> {A, B, C} end,
    timer:sleep(50),
    undefined = ets:info(T1, size),
    undefined = ets:info(T2, size),
    undefined = ets:info(T3, size),
    ok.

%% ---------------------------------------------------------------------------
%% Test 4: give_away transfers ownership; new owner sees itself as owner.
%% ---------------------------------------------------------------------------
test_give_away_transfers_ownership() ->
    Tab = ets:new(give_test_1, [set, public]),
    ets:insert(Tab, {key, value}),
    Parent = self(),
    Child = spawn(fun() ->
        receive
            {check_owner, Tab2} ->
                Owner = ets:info(Tab2, owner),
                Parent ! {owner_result, Owner}
        end
    end),
    true = ets:give_away(Tab, Child, gift),
    Child ! {check_owner, Tab},
    receive
        {owner_result, Child} -> ok
    end,
    %% Clean up: child is now the owner, but we can still delete (public table)
    ets:delete(Tab),
    ok.

%% ---------------------------------------------------------------------------
%% Test 5: After give_away, original owner death does NOT delete the table.
%% ---------------------------------------------------------------------------
test_give_away_owner_death() ->
    Parent = self(),
    %% Spawn a process that creates a table and gives it away to us
    spawn(fun() ->
        Tab = ets:new(give_test_2, [set, public]),
        ets:insert(Tab, {hello, world}),
        true = ets:give_away(Tab, Parent, gift),
        Parent ! {gave_away, Tab},
        %% This process now exits — table should survive since we gave it away
        ok
    end),
    Tab = receive {gave_away, T} -> T end,
    timer:sleep(50),
    %% Table must still exist because ownership was transferred to us
    [{hello, world}] = ets:lookup(Tab, hello),
    Self = self(),
    Self = ets:info(Tab, owner),
    ets:delete(Tab),
    ok.

%% ---------------------------------------------------------------------------
%% Test 6: Linked process cascade — table deleted when owner killed via link.
%% ---------------------------------------------------------------------------
test_linked_cascade_deletes_table() ->
    Parent = self(),
    %% Spawn Owner process that creates a table and links to a Crasher
    spawn(fun() ->
        Tab = ets:new(cascade_test, [set, public]),
        ets:insert(Tab, {data, 42}),
        Parent ! {table_ready, Tab},
        %% Wait for signal to link to crasher
        receive {link_to, CrasherPid} ->
            link(CrasherPid),
            Parent ! linked,
            %% Wait until killed by link cascade
            receive after 5000 -> ok end
        end
    end),
    Tab = receive {table_ready, T} -> T end,
    %% Spawn a crasher that will exit with an error
    Crasher = spawn(fun() ->
        receive crash -> error(boom) end
    end),
    %% Tell owner to link to crasher
    %% We need the owner PID to send messages
    %% Actually, let's restructure: owner already sent table_ready, we need its PID
    %% Let's use a simpler approach: the first spawn sends its PID too
    ok = test_linked_cascade_deletes_table_2(Tab),
    ok.

%% Helper for test 6 — cleaner version with PID tracking
test_linked_cascade_deletes_table_2(_OldTab) ->
    Parent = self(),
    Owner = spawn(fun() ->
        Tab = ets:new(cascade_test_2, [set, public]),
        ets:insert(Tab, {data, 42}),
        Parent ! {my_table, Tab},
        receive {link_to, Pid} ->
            link(Pid),
            Parent ! linked_ok,
            receive after 5000 -> ok end
        end
    end),
    Tab = receive {my_table, T} -> T end,
    Crasher = spawn(fun() ->
        receive crash -> error(deliberate_crash) end
    end),
    Owner ! {link_to, Crasher},
    receive linked_ok -> ok end,
    %% Now crash the crasher — should cascade to Owner via link
    Crasher ! crash,
    timer:sleep(100),
    %% Table should be gone because Owner was killed by link cascade
    undefined = ets:info(Tab, size),
    ok.

%% ---------------------------------------------------------------------------
%% Test 7: Non-owner process death does NOT affect the table.
%% ---------------------------------------------------------------------------
test_non_owner_death_no_effect() ->
    Tab = ets:new(non_owner_test, [set, public]),
    ets:insert(Tab, {key, value}),
    Parent = self(),
    %% Spawn a process that uses the table but doesn't own it
    spawn(fun() ->
        [{key, value}] = ets:lookup(Tab, key),
        ets:insert(Tab, {extra, data}),
        Parent ! user_done,
        ok
    end),
    receive user_done -> ok end,
    timer:sleep(50),
    %% Table must still exist — the non-owner died, not the owner
    [{key, value}] = ets:lookup(Tab, key),
    [{extra, data}] = ets:lookup(Tab, extra),
    ets:delete(Tab),
    ok.

%% ---------------------------------------------------------------------------
%% Test 8: Owner death while another process is actively reading — no crash.
%% ---------------------------------------------------------------------------
test_owner_death_concurrent_access() ->
    Parent = self(),
    %% Spawn an owner that populates a table
    Owner = spawn(fun() ->
        Tab = ets:new(concurrent_death, [set, public]),
        lists:foreach(fun(I) ->
            ets:insert(Tab, {I, I * 10})
        end, lists:seq(1, 100)),
        Parent ! {table_ready_2, Tab},
        receive die -> ok end
    end),
    Tab = receive {table_ready_2, T} -> T end,
    %% Spawn a reader that reads in a loop
    Reader = spawn(fun() ->
        concurrent_reader_loop(Tab, 200),
        Parent ! reader_done
    end),
    %% Kill the owner while reader is active
    timer:sleep(10),
    Owner ! die,
    %% Wait for reader to finish — it should not crash
    receive reader_done -> ok end,
    ok.

concurrent_reader_loop(_, 0) -> ok;
concurrent_reader_loop(Tab, N) ->
    Key = (N rem 100) + 1,
    %% Lookup may return [] if table was deleted, that's fine
    _ = ets:lookup(Tab, Key),
    concurrent_reader_loop(Tab, N - 1).
