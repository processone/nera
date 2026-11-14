%% EXPECT: resume exceptions ok
-module(resume_exception).
-export([test/0]).

test() ->
    {'EXIT', timeout} = (catch delayed(exit)),
    thrown = (catch delayed(throw)),
    {'EXIT', {bad, Trace}} = (catch outer(error)),
    true = is_list(Trace),
    undefined = get(unreachable),
    handled = try outer(exit) catch exit:timeout -> handled end,
    handled = try delayed(error) catch error:bad ->
        receive after 1 -> handled end
    after put(cleanup, done) end,
    done = get(cleanup),
    Parent = self(),
    spawn(fun() -> receive after 5 -> Parent ! go end end),
    {'EXIT', awakened} = (catch message_exit()),
    ok = delayed(normal),
    %% An arithmetic operand's callee suspends, then raises after receiving.
    %% The outer arithmetic context must unwind, not resume its next effect.
    Self = self(),
    {ArithmeticPid, ArithmeticRef} = spawn_monitor(fun() ->
        put(arithmetic_cleanup, 0),
        {error, operand_failed} = try
            _ = 1 + arithmetic_operand(Self),
            put(arithmetic_unreachable, ran),
            wrong
        catch C:E -> {C, E}
        after put(arithmetic_cleanup, get(arithmetic_cleanup) + 1)
        end,
        undefined = get(arithmetic_unreachable),
        1 = erase(arithmetic_cleanup),
        undefined = get(arithmetic_cleanup),
        Self ! arithmetic_done
    end),
    receive {arithmetic_waiting, ArithmeticPid} -> ok end,
    await_arithmetic_suspension(ArithmeticPid),
    ArithmeticPid ! arithmetic_go,
    receive arithmetic_done -> ok end,
    receive {'DOWN', ArithmeticRef, process, ArithmeticPid, normal} -> ok end,
    io:format("resume exceptions ok~n").

outer(Kind) ->
    delayed(Kind),
    put(unreachable, true).

delayed(Kind) ->
    receive after 1 ->
        case Kind of
            exit -> exit(timeout);
            throw -> throw(thrown);
            error -> error(bad);
            normal -> ok
        end
    end.

arithmetic_operand(Parent) ->
    Parent ! {arithmetic_waiting, self()},
    receive arithmetic_go -> error(operand_failed) end.

%% Observe real waiting before sending: no timing assumption can substitute
%% for the callee actually suspending. The fixture runner bounds completion.
await_arithmetic_suspension(Pid) ->
    case process_info(Pid, status) of
        {status, waiting} -> ok;
        undefined -> error(arithmetic_exited_before_suspension);
        _ -> await_arithmetic_suspension(Pid)
    end.

message_exit() ->
    receive go -> exit(awakened) end.
