-module(list_ops).
-export([start/0, reverse/1, length_of/1, append/2, map_double/1]).

%% Tests list operations: building lists, pattern matching on lists,
%% recursive list processing.

start() ->
    %% Test reverse
    R1 = reverse([1, 2, 3]),
    io:format("reverse([1,2,3]) = ~w~n", [R1]),

    %% Test length
    L1 = length_of([10, 20, 30, 40]),
    io:format("length([10,20,30,40]) = ~w~n", [L1]),

    %% Test append
    A1 = append([1, 2], [3, 4]),
    io:format("append([1,2],[3,4]) = ~w~n", [A1]),

    %% Test map_double
    M1 = map_double([5, 10, 15]),
    io:format("map_double([5,10,15]) = ~w~n", [M1]),

    io:format("List ops test passed~n").

reverse(L) ->
    reverse(L, []).

reverse([], Acc) -> Acc;
reverse([H|T], Acc) ->
    reverse(T, [H|Acc]).

length_of([]) -> 0;
length_of([_|T]) ->
    1 + length_of(T).

append([], B) -> B;
append([H|T], B) ->
    [H | append(T, B)].

map_double([]) -> [];
map_double([H|T]) ->
    [H * 2 | map_double(T)].
