-module(selective_receive).
-export([test/0]).

%% Test selective receive: non-matching messages stay in the mailbox.

id(I) -> I.

test() ->
    %% Test 1: Skip first message, match second
    self() ! id(a),
    self() ! id(b),
    b = receive
        b -> b
    after 1000 -> timeout
    end,
    %% 'a' should still be in the mailbox
    a = receive
        X1 -> X1
    after 1000 -> timeout
    end,

    %% Test 2: Multiple non-matching messages before the match
    self() ! id(1),
    self() ! id(2),
    self() ! id(3),
    self() ! id(target),
    target = receive
        target -> target
    after 1000 -> timeout
    end,
    %% Remaining: 1, 2, 3 (in order)
    1 = receive M1 -> M1 after 1000 -> timeout end,
    2 = receive M2 -> M2 after 1000 -> timeout end,
    3 = receive M3 -> M3 after 1000 -> timeout end,

    %% Test 3: No match → timeout, messages preserved
    self() ! id(nomatch),
    timeout = receive
        specific_thing -> found
    after 0 -> timeout
    end,
    %% 'nomatch' should still be there
    nomatch = receive M4 -> M4 after 1000 -> timeout end,

    %% Test 4: Pattern match on tuple — skip non-tuples
    self() ! id(plain_atom),
    self() ! id({tagged, 42}),
    {tagged, 42} = receive
        {tagged, V} -> {tagged, V}
    after 1000 -> timeout
    end,
    plain_atom = receive M5 -> M5 after 1000 -> timeout end,

    ok.
