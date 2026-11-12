-module(tcp_reactor_close_active).
-export([test/0]).

%% Regression test for reactor stale-fd bug:
%% Closing a socket while {active, true/once} is set can leave stale
%% registrations in the reactor because:
%% 1. PENDING_COUNT doesn't track active_regs (active registrations)
%% 2. socket::remove() deletes the socket BEFORE the reactor processes
%%    the Deregister command, so the reactor can't deregister from poll
%%
%% Bug details (see reactor.rs):
%% - handle_register_active() adds to active_regs but never increments
%%   PENDING_COUNT, so has_pending() returns false even with live
%%   active registrations
%% - socket::remove() (socket.rs:98) sends async Deregister to reactor
%%   then immediately removes from registry. When reactor processes
%%   deregister_handle(), get_raw_fd() returns None so
%%   poll.registry().deregister() is never called
%% - should_terminate() (scheduler.rs:1015) calls has_pending() which
%%   misses active_regs, leading to premature shutdown detection

test() ->
    ok = test_close_active_true(),
    ok = test_close_active_once(),
    halt(0).

test_close_active_true() ->
    Self = self(),
    Pid = spawn(fun() ->
        close_active_loop(true, 3),
        Self ! {done, self()}
    end),
    receive
        {done, Pid} -> ok
    after 3000 ->
        halt(1)
    end.

test_close_active_once() ->
    Self = self(),
    Pid = spawn(fun() ->
        close_active_loop(once, 3),
        Self ! {done, self()}
    end),
    receive
        {done, Pid} -> ok
    after 3000 ->
        halt(1)
    end.

close_active_loop(_Mode, 0) -> ok;
close_active_loop(Mode, N) ->
    {ok, LSock} = gen_tcp:listen(0, [{active, false}, binary]),
    {ok, Port} = inet:port(LSock),
    {ok, CSock} = gen_tcp:connect({127,0,0,1}, Port, [{active, false}, binary]),
    {ok, SSock} = gen_tcp:accept(LSock),
    ok = gen_tcp:close(LSock),
    ok = inet:setopts(SSock, [{active, Mode}]),
    ok = gen_tcp:send(CSock, <<"data">>),
    ok = gen_tcp:close(SSock),
    ok = gen_tcp:close(CSock),
    close_active_loop(Mode, N - 1).
