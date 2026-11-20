%% EXPECT: float_conversion_badarg_ok
%% Regression: float/1 of an integer that rounds beyond the float range and
%% binary_to_term/1 of an encoded infinite or NaN float returned an infinite
%% or NaN value natively, which is not an Erlang float. BEAM raises
%% error:badarg for both. Integers and encoded floats at the edge of the
%% float range still convert. Run this fixture on native Nera and BEAM.
-module(float_conversion_badarg).
-export([test/0]).

test() ->
    Max = 1.7976931348623157e308,
    %% The integer halfway between the largest float and 2^1024 rounds to
    %% even, which is 2^1024: beyond the range. One below it rounds to Max.
    Halfway = (1 bsl 1024) - (1 bsl 970),
    Beyond = [1 bsl 1100, -(1 bsl 1100), 1 bsl 1024, (1 bsl 1024) - 1,
              Halfway, -Halfway],
    lists:foreach(
        fun(N) ->
            {error, badarg} = protect(fun() -> float(id(N)) end),
            {error, badarg} = protect(fun() -> erlang:float(id(N)) end),
            {error, badarg} = protect(fun() -> apply(erlang, float, [id(N)]) end),
            {error, badarg} = protect(fun() -> (fun erlang:float/1)(id(N)) end)
        end, Beyond),
    Max = float(id(Halfway - 1)),
    -1.7976931348623157e308 = float(id(-(Halfway - 1))),
    8.98846567431158e307 = float(id(1 bsl 1023)),
    %% The conversion fails before any float arithmetic or comparison.
    {error, badarg} = protect(fun() -> float(id(1 bsl 1100)) + 1.0 end),
    {error, badarg} = protect(fun() -> float(id(1 bsl 1100)) > 1.0 end),
    %% In a guard, the test just fails.
    no = case id(1 bsl 1100) of B when float(B) > 0.0 -> yes; _ -> no end,
    yes = case id(Halfway - 1) of F when float(F) > 0.0 -> yes; _ -> no end,
    %% Encoded floats whose bits are infinity or NaN, alone or in a list.
    Rejected = [<<131, 70, 127, 240, 0, 0, 0, 0, 0, 0>>,
                <<131, 70, 255, 240, 0, 0, 0, 0, 0, 0>>,
                <<131, 70, 127, 248, 0, 0, 0, 0, 0, 0>>,
                <<131, 70, 127, 240, 0, 0, 0, 0, 0, 1>>,
                <<131, 108, 0, 0, 0, 2, 70, 63, 240, 0, 0, 0, 0, 0, 0,
                  70, 127, 240, 0, 0, 0, 0, 0, 0, 106>>],
    lists:foreach(
        fun(Bin) ->
            {error, badarg} = protect(fun() -> binary_to_term(id(Bin)) end),
            {error, badarg} = protect(fun() -> apply(erlang, binary_to_term, [id(Bin)]) end)
        end, Rejected),
    %% Finite encoded floats at the edges still decode.
    Max = binary_to_term(id(<<131, 70, 127, 239, 255, 255, 255, 255, 255, 255>>)),
    <<1:1, 0:63>> = <<(binary_to_term(id(<<131, 70, 128, 0, 0, 0, 0, 0, 0, 0>>)))/float>>,
    5.0e-324 = binary_to_term(id(<<131, 70, 0, 0, 0, 0, 0, 0, 0, 1>>)),
    [1.0, Max] = binary_to_term(id(term_to_binary([1.0, Max]))),
    %% Outside try, the process exits with badarg and runs nothing after.
    badarg = uncaught(fun() -> float(id(1 bsl 1100)) end),
    badarg = uncaught(fun() -> binary_to_term(id(hd(Rejected))) end),
    nothing = receive Late -> {late, Late} after 100 -> nothing end,
    io:format("float_conversion_badarg_ok~n"),
    ok.

protect(F) ->
    try F() of
        Value -> {value, Value}
    catch
        Class:Reason -> {Class, Reason}
    end.

%% Runs F in a new process that sends a message after F returns, and gives
%% its exit reason without the stack.
uncaught(F) ->
    Parent = self(),
    {Pid, Ref} = spawn_monitor(fun() -> Result = F(), Parent ! {ran_on, Result} end),
    receive
        {'DOWN', Ref, process, Pid, {Reason, Stack}} when is_list(Stack) -> Reason;
        {'DOWN', Ref, process, Pid, Reason} -> Reason
    after 5000 -> timeout
    end.

id(X) -> X.
