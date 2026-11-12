%% Adapted from "Programming Erlang" by Joe Armstrong — clock pattern.
%% Tests periodic timer with receive...after and registered process.
-module(jaerlang_clock).
-export([test/0]).

id(I) -> I.

test() ->
    %% Start a clock that ticks every 50ms and increments a counter
    Parent = self(),
    Counter = spawn(fun() -> counter_loop(0, Parent) end),
    register(test_clock, spawn(fun() -> tick(id(50), Counter) end)),
    %% Scheduling can delay ticks on a busy machine. Wait for evidence that
    %% the periodic timer fired twice instead of assuming a wall-clock rate.
    receive
        two_ticks -> ok
    after 5000 ->
        error(clock_did_not_tick)
    end,
    %% Stop the clock
    test_clock ! {stop, self()},
    receive
        stopped -> ok
    after 5000 ->
        error(clock_did_not_stop)
    end,
    %% Read the counter
    Counter ! {self(), read},
    receive
        {count, N} ->
            %% The counter acknowledged at least two ticks before shutdown.
            true = (N >= 2),
            ok
    end.

tick(Time, Counter) ->
    receive
        {stop, From} ->
            From ! stopped
    after Time ->
        Counter ! tick,
        tick(Time, Counter)
    end.

counter_loop(N, Parent) ->
    receive
        tick ->
            case N + 1 of
                2 -> Parent ! two_ticks;
                _ -> ok
            end,
            counter_loop(N + 1, Parent);
        {From, read} ->
            From ! {count, N},
            ok
    end.
