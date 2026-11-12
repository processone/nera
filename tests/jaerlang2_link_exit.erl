%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (edemo1.erl)
%% Tests: spawn_link, link/1, process_flag(trap_exit, true), EXIT signal propagation chain
%%
%% Topology: A <--link-- B <--link-- C
%%   A traps exits. B optionally traps exits. C dies.
%%   We verify that EXIT signals propagate correctly through the chain.
%%
%% Note: A and B signal readiness to Main before C is spawned, ensuring
%% that link(A) and process_flag(trap_exit, true) have completed before
%% the EXIT propagation chain fires.
-module(jaerlang2_link_exit).
-export([test/0]).

test() ->
    %% Test 1: C dies with reason 'bye', B does NOT trap exits
    %% Expected: B dies (cascading EXIT), A receives {'EXIT', B, bye}
    test_chain(false, bye),

    %% Test 2: C dies with reason 'normal'
    %% Expected: B stays alive (normal exit doesn't kill linked processes
    %% unless trap_exit is on), A should not get an EXIT from B
    test_normal_exit(),

    %% Test 3: C dies with reason 'crash', B DOES trap exits
    %% Expected: B receives {'EXIT', C, crash} and stays alive.
    %%           A does NOT get an EXIT (B is still alive).
    test_chain_trap(true, crash),

    ok.

test_chain(BTrap, CReason) ->
    Self = self(),

    %% Spawn A: traps exits, reports what it receives
    A = spawn(fun() ->
                  process_flag(trap_exit, true),
                  Self ! a_ready,
                  receive
                      {'EXIT', _From, Reason} ->
                          Self ! {a_got_exit, Reason}
                  after 2000 ->
                      Self ! {a_got_exit, timeout}
                  end
              end),

    %% Spawn B: linked to A, optionally traps exits
    B = spawn(fun() ->
                  process_flag(trap_exit, BTrap),
                  link(A),
                  Self ! b_ready,
                  receive
                      {'EXIT', _From, _Reason} ->
                          %% If trapping, we'd handle it, but BTrap=false here
                          ok
                  after 2000 ->
                      ok
                  end
              end),

    %% Wait for A and B to be fully set up before spawning C
    receive a_ready -> ok after 2000 -> exit(a_not_ready) end,
    receive b_ready -> ok after 2000 -> exit(b_not_ready) end,

    %% Spawn C: linked to B, exits immediately with CReason
    spawn(fun() ->
              link(B),
              exit(CReason)
          end),

    %% A should receive the cascading EXIT from B
    Result = receive {a_got_exit, R} -> R after 3000 -> timeout end,
    bye = Result,
    ok.

test_normal_exit() ->
    Self = self(),

    %% A traps exits
    A = spawn(fun() ->
                  process_flag(trap_exit, true),
                  Self ! a_ready,
                  receive
                      {'EXIT', _From, Reason} ->
                          Self ! {a_got_exit, Reason}
                  after 500 ->
                      Self ! a_no_exit
                  end
              end),

    %% B does not trap exits, linked to A
    B = spawn(fun() ->
                  link(A),
                  Self ! b_ready,
                  receive
                  after 2000 -> ok
                  end
              end),

    %% Wait for A and B to be fully set up before spawning C
    receive a_ready -> ok after 2000 -> exit(a_not_ready) end,
    receive b_ready -> ok after 2000 -> exit(b_not_ready) end,

    %% C linked to B, exits normally
    spawn(fun() ->
              link(B),
              %% normal exit: just return
              ok
          end),

    %% In Erlang, a normal exit does NOT kill linked processes
    %% (unless they trap exits). So B stays alive, and A gets no EXIT.
    Result = receive
        {a_got_exit, _} -> unexpected;
        a_no_exit -> ok
    after 3000 -> timeout
    end,
    ok = Result,
    ok.

test_chain_trap(BTrap, CReason) ->
    Self = self(),

    %% A traps exits
    A = spawn(fun() ->
                  process_flag(trap_exit, true),
                  Self ! a_ready,
                  receive
                      {'EXIT', _From, Reason} ->
                          Self ! {a_got_exit, Reason}
                  after 500 ->
                      Self ! a_no_exit
                  end
              end),

    %% B traps exits, linked to A.
    %% B must stay alive long enough for A's timeout to expire,
    %% otherwise B's normal termination sends {'EXIT', B, normal} to A.
    BPid = spawn(fun() ->
                   process_flag(trap_exit, BTrap),
                   link(A),
                   Self ! b_ready,
                   receive
                       {'EXIT', _From, _Reason} ->
                           %% B catches C's exit, stays alive
                           Self ! b_caught_exit,
                           %% Keep B alive while A checks for no EXIT
                           receive done -> ok after 3000 -> ok end
                   after 5000 ->
                       ok
                   end
               end),

    %% Wait for A and B to be fully set up before spawning C
    receive a_ready -> ok after 2000 -> exit(a_not_ready) end,
    receive b_ready -> ok after 2000 -> exit(b_not_ready) end,

    %% Spawn C linked to B, exits with CReason
    spawn(fun() ->
              link(BPid),
              exit(CReason)
          end),

    %% B should catch the EXIT
    b_caught_exit = receive BMsg -> BMsg after 2000 -> b_timeout end,

    %% A should NOT get an EXIT (B is alive and caught it)
    AResult = receive {a_got_exit, _} -> unexpected_exit; a_no_exit -> ok after 1000 -> ok end,
    ok = AResult,

    %% Let B terminate
    BPid ! done,

    ok.
