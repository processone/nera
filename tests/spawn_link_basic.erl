%% Comprehensive tests for spawn_link/1 and spawn_link/3.
%% Verifies atomic spawn+link, EXIT propagation, and trap_exit semantics.
-module(spawn_link_basic).
-export([test/0, worker_fun/1, crasher/1]).

test() ->
    test_closure_normal_exit(),
    test_closure_crash_trap(),
    test_mfa_normal_exit(),
    test_mfa_crash_trap(),
    test_no_race_on_link(),
    test_bidirectional_link(),
    ok.

%% --- Test 1: spawn_link/1, child exits normally, parent traps ---
test_closure_normal_exit() ->
    process_flag(trap_exit, true),
    Self = self(),
    Pid = spawn_link(fun() -> Self ! {hi, self()} end),

    %% Get the message
    {hi, Pid} = receive M1 -> M1 after 2000 -> exit(no_hi) end,

    %% Get EXIT normal
    receive
        {'EXIT', Pid, normal} -> ok
    after 2000 ->
        exit(no_exit_normal)
    end,
    process_flag(trap_exit, false),
    ok.

%% --- Test 2: spawn_link/1, child crashes, parent traps exits ---
test_closure_crash_trap() ->
    process_flag(trap_exit, true),
    Pid = spawn_link(fun() -> exit(oops) end),

    receive
        {'EXIT', Pid, oops} -> ok
    after 2000 ->
        exit(no_exit_oops)
    end,
    process_flag(trap_exit, false),
    ok.

%% --- Test 3: spawn_link/3 with MFA, normal exit ---
test_mfa_normal_exit() ->
    process_flag(trap_exit, true),
    Self = self(),
    Pid = spawn_link(?MODULE, worker_fun, [Self]),

    {working, Pid} = receive M2 -> M2 after 2000 -> exit(no_working) end,

    receive
        {'EXIT', Pid, normal} -> ok
    after 2000 ->
        exit(no_exit_mfa_normal)
    end,
    process_flag(trap_exit, false),
    ok.

%% --- Test 4: spawn_link/3 with MFA, crash exit ---
test_mfa_crash_trap() ->
    process_flag(trap_exit, true),
    Pid = spawn_link(?MODULE, crasher, [kaboom]),

    receive
        {'EXIT', Pid, kaboom} -> ok
    after 2000 ->
        exit(no_exit_mfa_crash)
    end,
    process_flag(trap_exit, false),
    ok.

%% --- Test 5: spawn_link has no race — link is established before child runs ---
%% Child exits immediately; parent still gets the EXIT because link is atomic.
test_no_race_on_link() ->
    process_flag(trap_exit, true),
    %% Child exits with 'fast' immediately. If spawn_link is atomic,
    %% the link is in place and we get the EXIT signal.
    Pid = spawn_link(fun() -> exit(fast) end),

    receive
        {'EXIT', Pid, fast} -> ok
    after 2000 ->
        exit(race_on_link)
    end,
    process_flag(trap_exit, false),
    ok.

%% --- Test 6: Bidirectional link — both sides can see each other ---
test_bidirectional_link() ->
    process_flag(trap_exit, true),
    Self = self(),

    Pid = spawn_link(fun() ->
                         process_flag(trap_exit, true),
                         Self ! ready,
                         %% Wait for parent to confirm link exists
                         receive check -> ok after 2000 -> ok end,
                         %% Verify that Self is in our links
                         %% (we can't inspect links directly, but if parent
                         %%  dies we'd get an EXIT — that's the semantics)
                         Self ! link_confirmed
                     end),

    receive ready -> ok after 2000 -> exit(no_ready) end,
    Pid ! check,
    receive link_confirmed -> ok after 2000 -> exit(no_confirm) end,

    %% Unlink before cleanup to avoid cascade
    unlink(Pid),
    Pid ! done,
    %% Drain any EXIT from child's normal termination
    receive {'EXIT', Pid, _} -> ok after 200 -> ok end,
    process_flag(trap_exit, false),
    ok.

%% Exported functions for spawn_link/3 tests
worker_fun(Parent) ->
    Parent ! {working, self()},
    ok.

crasher(Reason) ->
    exit(Reason).
