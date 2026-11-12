-module(tcp_socket_cleanup).
-export([test/0]).

%% Test that TCP listen/close works correctly.
%% This is a basic sanity test for the Arc-based socket registry.

test() ->
    %% Basic listen + close
    {ok, LSock} = gen_tcp:listen(0, []),
    ok = gen_tcp:close(LSock),

    %% Multiple listen + close cycles (verify no FD leak)
    ok = listen_close_loop(20),

    ok.

listen_close_loop(0) -> ok;
listen_close_loop(N) ->
    {ok, LSock} = gen_tcp:listen(0, []),
    ok = gen_tcp:close(LSock),
    listen_close_loop(N - 1).
