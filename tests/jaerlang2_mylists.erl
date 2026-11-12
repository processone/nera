%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (mylists.erl)
%% Tests: higher-order functions (map), list recursion, pattern matching, accumulator pattern
-module(jaerlang2_mylists).
-export([test/0]).

test() ->
    %% map
    [2, 4, 6] = map(fun(X) -> X * 2 end, [1, 2, 3]),
    [] = map(fun(X) -> X end, []),

    %% member
    true  = member(2, [1, 2, 3]),
    false = member(4, [1, 2, 3]),
    false = member(1, []),

    %% sum
    0  = sum([]),
    6  = sum([1, 2, 3]),
    10 = sum([10]),

    %% reverse
    []        = reverse([]),
    [3, 2, 1] = reverse([1, 2, 3]),
    [1]       = reverse([1]),

    %% partition
    {[1, 3, 5], [2, 4]} = partition(fun(X) -> (X rem 2) =:= 1 end, [1, 2, 3, 4, 5]),

    %% search
    {ok, banana} = search(b, [{a, apple}, {b, banana}, {c, cherry}]),
    error = search(z, [{a, apple}]),

    ok.

map(_, [])     -> [];
map(F, [H|T])  -> [F(H)|map(F, T)].

member(H, [H|_]) -> true;
member(H, [_|T]) -> member(H, T);
member(_, [])     -> false.

sum([H|T]) -> H + sum(T);
sum([])    -> 0.

reverse(L) -> reverse(L, []).
reverse([H|T], Acc) -> reverse(T, [H|Acc]);
reverse([], Acc)    -> Acc.

partition(Pred, L) -> partition(Pred, L, [], []).
partition(Pred, [H|T], Yes, No) ->
    case Pred(H) of
        true  -> partition(Pred, T, [H|Yes], No);
        false -> partition(Pred, T, Yes, [H|No])
    end;
partition(_, [], Yes, No) ->
    {reverse(Yes), reverse(No)}.

search(Key, [{Key, Val}|_]) -> {ok, Val};
search(Key, [_|T])          -> search(Key, T);
search(_, [])               -> error.
