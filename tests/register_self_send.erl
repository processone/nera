-module(register_self_send).
-export([test/0]).

%% Test: register self, then send to self via registered name.
%% This exercises the self-send deadlock avoidance path combined
%% with registered-name resolution.
test() ->
    Self = self(),
    true = register(me, Self),

    %% Self-send via registered name
    me ! hello,
    me ! {data, 42},

    %% Receive both messages
    hello = receive M1 -> M1 after 500 -> exit(timeout1) end,
    {data, 42} = receive M2 -> M2 after 500 -> exit(timeout2) end,

    %% Also test whereis on self-registered name
    Self = whereis(me),

    unregister(me),
    ok.
