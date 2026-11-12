-module(simple_list).
-export([start/0, reverse/1, reverse/2]).

start() ->
    R = reverse([1, 2, 3]),
    io:format("reverse: ~w~n", [R]).

reverse(L) ->
    reverse(L, []).

reverse([], Acc) -> Acc;
reverse([H|T], Acc) ->
    reverse(T, [H|Acc]).
