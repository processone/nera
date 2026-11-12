-module(tcp_accept_kill).
-export([test/0]).

%% Test that a process blocked in gen_tcp:accept can be killed
%% and the runtime terminates cleanly.

test() ->
    {ok, LSock} = gen_tcp:listen(0, []),
    Self = self(),

    %% Spawn a process that blocks on accept
    Pid = spawn(fun() ->
        Self ! ready,
        gen_tcp:accept(LSock)
    end),

    %% Wait for it to start
    receive ready -> ok end,

    %% Give it time to enter IO_WAIT
    receive after 50 -> ok end,

    %% Kill it
    exit(Pid, kill),

    %% Wait for cleanup
    receive after 200 -> ok end,

    gen_tcp:close(LSock),
    ok.
