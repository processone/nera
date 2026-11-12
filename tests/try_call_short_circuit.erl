%% EXPECT: try_call_short_circuit_ok
-module(try_call_short_circuit).
-export([test/0]).
test() ->
    erase(after_exception),
    ok = try abs(not_a_number), put(after_exception, true)
         catch error:badarg -> ok end,
    undefined = get(after_exception),
    io:format("try_call_short_circuit_ok~n").
