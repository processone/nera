%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (lists1.erl)
%% Tests: custom list functions - map, member, partition, reverse, search
-module(jaerlang2_lists).
-export([test/0]).

test() ->
    %% Test map
    [2, 4, 6] = map(fun(X) -> X * 2 end, [1, 2, 3]),
    [] = map(fun(X) -> X end, []),
    ["hello"] = map(fun(_) -> "hello" end, [1]),

    %% Test member
    true = member(3, [1, 2, 3, 4]),
    false = member(5, [1, 2, 3, 4]),
    false = member(a, []),
    true = member(a, [a]),

    %% Test reverse
    [3, 2, 1] = reverse([1, 2, 3]),
    [] = reverse([]),
    [a] = reverse([a]),

    %% Test partition
    {[2, 4, 6], [1, 3, 5]} = partition(fun(X) -> (X rem 2) =:= 0 end, [1, 2, 3, 4, 5, 6]),
    {[], [1, 3, 5]} = partition(fun(X) -> X > 10 end, [1, 3, 5]),
    {[], []} = partition(fun(_) -> true end, []),

    %% Test search (association list lookup)
    {ok, paris} = search(france, [{usa, washington}, {france, paris}, {uk, london}]),
    {ok, london} = search(uk, [{usa, washington}, {france, paris}, {uk, london}]),
    error = search(germany, [{usa, washington}, {france, paris}]),
    error = search(a, []),

    %% Test filter
    [4, 5, 6] = filter(fun(X) -> X > 3 end, [1, 2, 3, 4, 5, 6]),
    [] = filter(fun(X) -> X > 10 end, [1, 2, 3]),

    %% Test sum
    15 = sum([1, 2, 3, 4, 5]),
    0 = sum([]),
    42 = sum([42]),

    %% Test foreach (side-effect: send messages to self)
    Self = self(),
    foreach(fun(X) -> Self ! {item, X} end, [a, b, c]),
    {item, a} = receive M1 -> M1 end,
    {item, b} = receive M2 -> M2 end,
    {item, c} = receive M3 -> M3 end,

    ok.

%% Custom list functions adapted from Joe Armstrong's lists1.erl

map(_, []) -> [];
map(F, [H | T]) -> [F(H) | map(F, T)].

member(H, [H | _]) -> true;
member(H, [_ | T]) -> member(H, T);
member(_, []) -> false.

partition(Pred, L) -> partition(Pred, L, [], []).

partition(Pred, [H | T], Ts, Fs) ->
    case Pred(H) of
        true  -> partition(Pred, T, [H | Ts], Fs);
        false -> partition(Pred, T, Ts, [H | Fs])
    end;
partition(_, [], Ts, Fs) ->
    {reverse(Ts), reverse(Fs)}.

reverse(L) -> reverse(L, []).

reverse([H | T], L) -> reverse(T, [H | L]);
reverse([], L) -> L.

search(Key, [{Key, Val} | _]) -> {ok, Val};
search(Key, [_ | T]) -> search(Key, T);
search(_, []) -> error.

filter(_, []) -> [];
filter(F, [H | T]) ->
    case F(H) of
        true -> [H | filter(F, T)];
        false -> filter(F, T)
    end.

sum(L) -> sum(L, 0).
sum([], N) -> N;
sum([H | T], N) -> sum(T, H + N).

foreach(_, []) -> ok;
foreach(F, [H | T]) ->
    F(H),
    foreach(F, T).
