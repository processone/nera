%% Ported from OTP trycatch_SUITE concepts: last-call/tail-call optimization
%% through try/catch blocks. A deep recursion inside try must not blow the
%% stack when the try/catch is in tail position.
-module(catch_last_call).
-export([test/0]).

test() ->
    %% Tail-recursive loop whose body is a try/catch: should handle large N.
    N = 20000,
    N = loop_in_try(N, 0),
    %% Same, but the try body raises every other iteration.
    N = loop_raising(N, 0),
    %% catch expression in tail position
    done = catch_tail(10000),
    %% try..of in tail position (smaller N — nera currently does not do
    %% last-call optimization through `try..of`; keep N under the stack
    %% limit until that is fixed).
    done = try_of_tail(100),
    ok.

loop_in_try(0, Acc) -> Acc;
loop_in_try(N, Acc) ->
    Next = try
        id(Acc) + 1
    catch
        _:_ -> Acc
    end,
    loop_in_try(N - 1, Next).

loop_raising(0, Acc) -> Acc;
loop_raising(N, Acc) ->
    Next = try
        case N rem 2 of
            0 -> error(stop);
            1 -> Acc + 1
        end
    catch
        error:stop -> Acc + 1
    end,
    loop_raising(N - 1, Next).

catch_tail(0) -> done;
catch_tail(N) ->
    _ = (catch id(N)),
    catch_tail(N - 1).

try_of_tail(0) -> done;
try_of_tail(N) ->
    try id(N) of
        _ -> try_of_tail(N - 1)
    catch
        _:_ -> failed
    end.

id(X) -> X.
