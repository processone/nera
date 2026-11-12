%% Ported from OTP trycatch_SUITE concepts: catch/try around short-circuit
%% boolean operators and if expressions.
-module(catch_andalso_orelse).
-export([test/0]).

test() ->
    %% andalso: short-circuits, right-hand side never evaluated
    false = catch_andalso(false, fun() -> error(rhs_should_not_run) end),
    %% andalso: both sides evaluated
    true  = catch_andalso(true, fun() -> true end),
    false = catch_andalso(true, fun() -> false end),
    %% andalso: exception on right-hand side is caught
    {caught, boom} = catch_andalso_exn(true, fun() -> error(boom) end),
    %% orelse: short-circuits
    true  = catch_orelse(true, fun() -> error(rhs_should_not_run) end),
    %% orelse: both sides evaluated
    true  = catch_orelse(false, fun() -> true end),
    false = catch_orelse(false, fun() -> false end),
    %% orelse: exception on right-hand side is caught
    {caught, boom} = catch_orelse_exn(false, fun() -> error(boom) end),
    %% catch around the whole expression returns the boolean
    true = catch (1 < 2) andalso (2 < 3),
    false = catch (1 < 2) andalso (3 < 2),
    %% catch around an if expression
    big   = catch_if(10),
    small = catch_if(1),
    ok.

catch_andalso(L, RFun) ->
    try
        L andalso RFun()
    catch
        _:_ -> error
    end.

catch_andalso_exn(L, RFun) ->
    try
        L andalso RFun()
    catch
        error:R -> {caught, R}
    end.

catch_orelse(L, RFun) ->
    try
        L orelse RFun()
    catch
        _:_ -> error
    end.

catch_orelse_exn(L, RFun) ->
    try
        L orelse RFun()
    catch
        error:R -> {caught, R}
    end.

catch_if(N) ->
    V = catch (if N > 5 -> big; true -> small end),
    V.
