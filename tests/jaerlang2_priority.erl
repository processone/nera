%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (Ch. 12 priority receive)
%% Tests: selective receive skipping non-matching messages, priority message handling
-module(jaerlang2_priority).
-export([test/0]).

test() ->
    %% Test 1: Priority messages are received before normal messages.
    %% Send normal messages first, then a priority message.
    self() ! {normal, 1},
    self() ! {normal, 2},
    self() ! {priority, high},
    self() ! {normal, 3},

    %% Selective receive picks {priority, _} first, skipping normals
    high = receive {priority, P} -> P end,

    %% Now receive normals in their original order
    1 = receive {normal, N1} -> N1 end,
    2 = receive {normal, N2} -> N2 end,
    3 = receive {normal, N3} -> N3 end,

    %% Test 2: Multiple priority levels - pick in desired order
    self() ! {low, a},
    self() ! {medium, b},
    self() ! {high, c},
    self() ! {low, d},

    %% Pick high first (skips low and medium)
    c = receive {high, H} -> H end,
    %% Then medium (skips low)
    b = receive {medium, M} -> M end,
    %% Then lows in order
    a = receive {low, L1} -> L1 end,
    d = receive {low, L2} -> L2 end,

    %% Test 3: Selective receive with interleaved tags
    self() ! {a, 1},
    self() ! {b, 2},
    self() ! {c, 3},
    self() ! {a, 4},

    %% Pick tag 'c' first (skipping a and b)
    3 = receive {c, V1} -> V1 end,
    %% Pick tag 'b' (skipping a)
    2 = receive {b, V2} -> V2 end,
    %% Remaining 'a' messages in order
    1 = receive {a, V3} -> V3 end,
    4 = receive {a, V4} -> V4 end,

    ok.
