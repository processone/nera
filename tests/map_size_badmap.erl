%% EXPECT: map_size_badmap_ok
%% Regression: map_size/1 and maps:size/1 raise {badmap, Term} for a
%% non-map, as on BEAM, and a guard that calls map_size/1 on a non-map fails
%% instead of seeing a size of 0. Before the fix, map_size(foo) returned 0,
%% so `map_size(M) =:= 0` matched atoms, lists, tuples and binaries as if
%% they were empty maps. Run this same fixture on native Nera and BEAM.
-module(map_size_badmap).
-export([test/0]).

test() ->
    Sizes = [{id(#{}), 0}, {id(#{a => 1}), 1},
             {id(#{a => 1, b => 2, c => 3}), 3},
             {maps:from_list([{I, I} || I <- lists:seq(1, 100)]), 100}],
    lists:foreach(fun check_size/1, Sizes),
    <<_, Tail/binary>> = id(<<1, 2, 3>>),
    NonMaps = id([not_a_map, 0, 42, 1 bsl 70, 1.0, [], [a], [a | b], {},
                  {a}, <<>>, <<"a">>, Tail, self(), make_ref(),
                  fun() -> ok end]),
    lists:foreach(fun check_badmap/1, NonMaps),
    %% A failed guard clears the exception before the next guard alternative.
    recovered = alternative(not_a_map),
    {badmap, <<2, 3>>} = reason(fun() -> map_size(Tail) end),
    io:format("map_size_badmap_ok~n"),
    ok.

check_size({M, N}) ->
    N = map_size(M),
    N = erlang:map_size(M),
    N = maps:size(M),
    lists:foreach(fun(Call) -> N = Call(M) end, calls()),
    true = (empty(M) =:= (N =:= 0)),
    true = (qualified_empty(M) =:= (N =:= 0)),
    true = (non_empty(M) =:= (N > 0)),
    Class = expected_class(N),
    Class = class(M),
    Class = case_class(M),
    Class = if_class(M).

check_badmap(M) ->
    {badmap, M} = reason(fun() -> map_size(M) end),
    {badmap, M} = reason(fun() -> erlang:map_size(M) end),
    {badmap, M} = reason(fun() -> maps:size(M) end),
    lists:foreach(fun(Call) -> {badmap, M} = reason(fun() -> Call(M) end) end,
                  calls()),
    %% Each guard below would be true for a size of 0, so a non-map that
    %% passes it shows map_size/1 returned instead of raising.
    false = empty(M),
    false = qualified_empty(M),
    false = non_empty(M),
    false = not_non_empty(M),
    false = empty_orelse(M),
    false = empty_and_term(M),
    other = class(M),
    other = case_class(M),
    other = if_class(M),
    ok.

%% Every other way to reach the two functions: apply/3, a dynamic call and
%% external fun references.
calls() ->
    Erlang = id(erlang), MapSize = id(map_size),
    Maps = id(maps), Size = id(size),
    [fun(M) -> apply(Erlang, MapSize, [M]) end,
     fun(M) -> apply(Maps, Size, [M]) end,
     fun(M) -> Erlang:MapSize(M) end,
     fun(M) -> Maps:Size(M) end,
     fun erlang:map_size/1,
     fun maps:size/1,
     fun map_size/1].

empty(M) when map_size(M) =:= 0 -> true;
empty(_) -> false.
qualified_empty(M) when erlang:map_size(M) == 0 -> true;
qualified_empty(_) -> false.
non_empty(M) when map_size(M) > 0 -> true;
non_empty(_) -> false.
%% A raising call fails the whole guard test, so `not` does not turn it true.
not_non_empty(M) when not (map_size(M) > 0) -> true;
not_non_empty(_) -> false.
empty_orelse(M) when map_size(M) =:= 0 orelse true -> true;
empty_orelse(_) -> false.
empty_and_term(M) when map_size(M) =< 0, M =/= ok -> true;
empty_and_term(_) -> false.
alternative(M) when map_size(M) =:= 0; M =:= not_a_map -> recovered;
alternative(_) -> failed.

class(M) when map_size(M) =:= 0 -> empty;
class(M) when map_size(M) =< 3 -> small;
class(M) when is_map(M) -> large;
class(_) -> other.

case_class(M) ->
    case M of
        _ when map_size(M) =:= 0 -> empty;
        _ when map_size(M) =< 3 -> small;
        _ when is_map(M) -> large;
        _ -> other
    end.

if_class(M) ->
    if
        map_size(M) =:= 0 -> empty;
        map_size(M) =< 3 -> small;
        is_map(M) -> large;
        true -> other
    end.

expected_class(0) -> empty;
expected_class(N) when N =< 3 -> small;
expected_class(_) -> large.

reason(F) -> try F() of V -> {unexpected, V} catch error:R -> R end.
id(X) -> X.
