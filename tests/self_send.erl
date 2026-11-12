%% Regression test for self-send deadlock.
%% self() ! Msg used to deadlock because the worker thread held the
%% process Mutex and send_message tried to re-acquire it.
-module(self_send).
-export([test/0]).

test() ->
    %% Basic self-send
    self() ! hello,
    hello = receive M1 -> M1 after 1000 -> exit(no_hello) end,

    %% Multiple self-sends in sequence
    self() ! a,
    self() ! b,
    self() ! c,
    a = receive M2 -> M2 after 1000 -> exit(no_a) end,
    b = receive M3 -> M3 after 1000 -> exit(no_b) end,
    c = receive M4 -> M4 after 1000 -> exit(no_c) end,

    %% Self-send with complex term
    self() ! {tuple, [1,2,3], #{key => value}},
    {tuple, [1,2,3], #{key => value}} =
        receive M5 -> M5 after 1000 -> exit(no_complex) end,

    ok.
