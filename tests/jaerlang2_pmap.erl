%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (lib_misc.erl pmap/2)
%% Tests: spawn, make_ref, reference matching in receive, parallel execution
%%
%% Note: The original JA pmap uses selective receive (matching on specific PIDs)
%% to gather results in order. Since selective receive scanning past non-matching
%% messages is not yet supported, this version uses index-tagged results instead.
-module(jaerlang2_pmap).
-export([test/0]).

test() ->
    %% pmap: apply a function in parallel across a list
    %% Double each element
    [2, 4, 6, 8, 10] = pmap(fun(X) -> X * 2 end, [1, 2, 3, 4, 5]),

    %% Square each element
    [1, 4, 9, 16, 25] = pmap(fun(X) -> X * X end, [1, 2, 3, 4, 5]),

    %% Empty list
    [] = pmap(fun(X) -> X end, []),

    %% Single element
    [42] = pmap(fun(X) -> X * 6 end, [7]),

    %% Results preserve order
    [10, 20, 30] = pmap(fun(X) -> X * 10 end, [1, 2, 3]),

    ok.

%% Parallel map: spawns one process per element, gathers results in order.
%% Uses index-tagged messages so we don't depend on selective receive.
pmap(F, L) ->
    S = self(),
    Ref = make_ref(),
    N = length(L),
    spawn_indexed(S, Ref, F, L, 0),
    Results = gather_all(N, Ref),
    %% Sort by index and extract values
    extract_ordered(sort(Results), 0).

spawn_indexed(_, _, _, [], _) -> ok;
spawn_indexed(Parent, Ref, F, [H | T], Idx) ->
    spawn(fun() -> Parent ! {Ref, Idx, F(H)} end),
    spawn_indexed(Parent, Ref, F, T, Idx + 1).

gather_all(0, _Ref) -> [];
gather_all(N, Ref) ->
    receive
        {Ref, Idx, Val} ->
            [{Idx, Val} | gather_all(N - 1, Ref)]
    end.

%% Simple insertion sort for small lists of {Index, Value} pairs
sort([]) -> [];
sort([H | T]) -> insert(H, sort(T)).

insert(X, []) -> [X];
insert({Ix, _} = X, [{Iy, _} = Y | T]) when Ix =< Iy -> [X, Y | T];
insert(X, [Y | T]) -> [Y | insert(X, T)].

%% Extract values in index order
extract_ordered([], _) -> [];
extract_ordered([{_, Val} | T], N) ->
    [Val | extract_ordered(T, N + 1)].
