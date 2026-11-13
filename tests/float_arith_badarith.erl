%% EXPECT: float_arith_badarith_ok
%% Regression: `/` with a zero divisor and float arithmetic whose result does
%% not fit a float returned an infinite float natively instead of raising
%% error:badarith. Integers too large for a float raise badarith too when
%% float arithmetic needs them as floats. Results that stay finite, underflow
%% to zero included, are unchanged. A comprehension filter that is false
%% stops the later ones, so a filter that guards a division keeps working.
%% Run this fixture on native Nera and BEAM.
-module(float_arith_badarith).
-export([test/0]).

test() ->
    Huge = 1 bsl 1100,
    %% Zero divisors: integer and float, signed zeros, zero by zero.
    lists:foreach(fun({A, B}) -> {error, badarith} = protect(fun() -> id(A) / id(B) end) end,
                  [{1, 0}, {0, 0}, {-1, 0}, {1.0, 0.0}, {1.0, -0.0}, {0.0, 0.0},
                   {-0.0, 0.0}, {1, 0.0}, {1.0, 0}, {1.7976931348623157e308, 0.0}]),
    %% Results beyond the float range, either sign.
    Overflows =
        [fun() -> id(1.0e308) * 10.0 end,
         fun() -> id(-1.0e308) * 10.0 end,
         fun() -> id(1.0e308) * 10 end,
         fun() -> id(1.0e308) + 1.0e308 end,
         fun() -> id(-1.0e308) - 1.0e308 end,
         fun() -> id(1.0e308) - -1.0e308 end,
         fun() -> id(1.0e308) / 1.0e-10 end,
         fun() -> id(1.0e308) * (1 bsl 10) end,
         fun() -> lists:sum([id(1.0e308), 1.0e308]) end],
    lists:foreach(fun(F) -> {error, badarith} = protect(F) end, Overflows),
    %% Integers too large for a float, in float arithmetic.
    BigOperands =
        [fun() -> id(Huge) + 1.0 end,
         fun() -> 1.0 - id(Huge) end,
         fun() -> id(Huge) * 0.0 end,
         fun() -> 1 / id(Huge) end,
         fun() -> id(Huge) / 1 end,
         fun() -> id(Huge) / (1 bsl 1099) end],
    lists:foreach(fun(F) -> {error, badarith} = protect(F) end, BigOperands),
    %% Operands written as literals.
    {error, badarith} = protect(fun() -> 1.0e308 * 10.0 end),
    {error, badarith} = protect(fun() -> 1.0e308 + 1.0e308 end),
    {error, badarith} = protect(fun() -> 1.0e308 / 1.0e-10 end),
    %% Finite results are unchanged.
    3.5 = id(7) / 2,
    2.0 = id(1 bsl 1000) / (1 bsl 999),
    <<0:64>> = <<(id(1.0e-308) / 1.0e308)/float>>,
    <<0:64>> = <<(id(5.0e-324) / 2.0)/float>>,
    <<1:1, 0:63>> = <<(id(-0.0) / 1.0)/float>>,
    1.7976931348623157e308 = id(1.7976931348623157e308) * 1.0,
    1.0e308 = id(1.0e308) + 1.0,
    -1.0e308 = id(-1.0e308) * 1,
    %% Inside try, the error reaches the handler before a later send.
    {error, badarith} =
        try
            X = id(1.0) / 0.0,
            self() ! after_division,
            X
        catch
            C1:E1 -> {C1, E1}
        end,
    {error, badarith} =
        try
            Y = id(1.0e308) * 10.0,
            self() ! after_overflow,
            Y
        catch
            C2:E2 -> {C2, E2}
        end,
    nothing = receive Leaked -> {leaked, Leaked} after 0 -> nothing end,
    %% In a guard, the test just fails.
    no = case id(0.0) of Z when 1 / Z > 0 -> yes; _ -> no end,
    no = case id(10.0) of T when 1.0e308 * T > 0 -> yes; _ -> no end,
    %% A comprehension filter that is false stops the later filters, so a
    %% division or product they guard against never runs.
    [2.0] = [X || X <- id([0.0, 2.0]), X /= 0.0, id(1 / X) > 0],
    [2.0] = [X || X <- id([1.0e308, 2.0]), X < 1.0e300, id(X * 10.0) > 0],
    [3.0] = [X || X <- id([0.0, 1.0e308, 3.0]), X /= 0.0, X < 1.0e300,
                  id(1 / X) * id(X * 10.0) > 0],
    [{1.0, 2.0}] = [{X, Y} || X <- id([0.0, 1.0]), X /= 0.0, Y <- [2.0], id(Y / X) > 1],
    <<2>> = << <<X>> || <<X>> <= id(<<0, 2>>), X /= 0, id(1 / X) > 0 >>,
    [1] = [X || X <- id([1, 2, 3]), X < 2, mark(X)],
    [{marked, 1}] = drain(),
    %% Outside try, the process exits with badarith and runs nothing after.
    badarith = uncaught(fun() -> id(1.0) / 0.0 end),
    badarith = uncaught(fun() -> id(1.0e308) * 10.0 end),
    badarith = uncaught(fun() -> id(1.0e308) + 1.0e308 end),
    nothing = receive Late -> {late, Late} after 100 -> nothing end,
    io:format("float_arith_badarith_ok~n"),
    ok.

mark(X) ->
    self() ! {marked, X},
    true.

drain() ->
    drain([]).

drain(Messages) ->
    receive Message -> drain([Message | Messages]) after 0 -> lists:reverse(Messages) end.

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
