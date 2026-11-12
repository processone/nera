-module(register_send).
-export([test/0]).

%% Test: sending messages to registered names (Name ! Msg)
test() ->
    Self = self(),
    register(echo_server, Self),

    %% Spawn a process that sends to the registered name
    spawn(fun() ->
        echo_server ! {hello, 42}
    end),

    %% Receive the message
    receive
        {hello, 42} -> ok
    after 1000 ->
        exit(timeout)
    end,

    unregister(echo_server),
    ok.
