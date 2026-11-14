%% EXPECT: arithmetic_tuple_send_ok
%% Enlarging arithmetic blocks must not let tuple/send fusion replace a
%% tuple that is read by an earlier comparison, even when arithmetic folds.
%% Run the same regression on native Nera and BEAM.
-module(arithmetic_tuple_send).
-export([test/0]).

test() ->
    lists:foreach(fun(F) ->
        true = F(self(), 1),
        receive {a} -> ok end
    end, [fun binary/2, fun negate/2, fun bit_not/2, fun folded/2]),
    true = boolean_not(self(), true),
    receive {a} -> ok end,
    io:format("arithmetic_tuple_send_ok~n").

binary(P, X) ->
    try T = {a}, Eq = T =:= {a}, _ = X + 1, P ! T, Eq
    catch _:_ -> failed end.

negate(P, X) ->
    try T = {a}, Eq = T =:= {a}, _ = -X, P ! T, Eq
    catch _:_ -> failed end.

bit_not(P, X) ->
    try T = {a}, Eq = T =:= {a}, _ = bnot X, P ! T, Eq
    catch _:_ -> failed end.

boolean_not(P, X) ->
    try T = {a}, Eq = T =:= {a}, _ = not X, P ! T, Eq
    catch _:_ -> failed end.

folded(P, _X) ->
    try T = {a}, Eq = T =:= {a}, _ = 1 + 1, P ! T, Eq
    catch _:_ -> failed end.
