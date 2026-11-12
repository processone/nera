-module(named_fun).
-export([test/0]).

test() ->
    %% Simple factorial with named fun (multi-clause)
    Fact = fun Factorial(0) -> 1;
               Factorial(N) -> N * Factorial(N - 1)
           end,
    1 = Fact(0),
    1 = Fact(1),
    6 = Fact(3),
    120 = Fact(5),

    %% Single-clause named fun
    Double = fun D(X) -> X * 2 end,
    10 = Double(5),
    0 = Double(0),

    %% Named fun with closure capture
    Base = 10,
    AddBase = fun Loop(0) -> Base;
                  Loop(N) -> N + Loop(N - 1)
              end,
    10 = AddBase(0),
    11 = AddBase(1),
    13 = AddBase(2),

    %% Named fun with guard
    ClampedFact = fun F(0) -> 1;
                      F(N) when N > 0 -> N * F(N - 1)
                  end,
    1 = ClampedFact(0),
    6 = ClampedFact(3),

    %% Non-recursive named fun (name unused but valid)
    Identity = fun Self(X) -> X end,
    hello = Identity(hello),
    42 = Identity(42),

    %% Named fun assigned and called multiple times
    Countdown = fun CD(0) -> done;
                    CD(N) -> CD(N - 1)
                end,
    done = Countdown(0),
    done = Countdown(5),
    done = Countdown(10),

    ok.
