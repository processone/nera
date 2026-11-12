%% Tests exception propagation through the tail call trampoline.
-module(tco_trampoline_exception).
-export([test/0, crasher/1]).

test() ->
    %% Dynamic tail call that raises an exception
    Result = try
        M = tco_trampoline_exception,
        M:crasher(boom)
    catch
        error:boom -> caught
    end,
    caught = Result,
    ok.

crasher(Reason) -> error(Reason).
