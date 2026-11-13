%% EXPECT: list_operator_contracts_ok
-module(list_operator_contracts).
-export([test/0]).
test() ->
    bad(fun() -> append([1|bad], [2]) end),
    bad(fun() -> append(bad, []) end),
    lists:foreach(fun({A,B}) -> bad(fun() -> subtract(A,B) end) end,
                  [{[1|bad],[]}, {[],[1|bad]}, {bad,[]}, {[],bad}, {[1|bad],[1]}]),
    [1,2|tail] = append([1], [2|tail]),
    tail = append([], tail),
    [2,1,1] = subtract([1,2,1,1], [1]),
    Ref = make_ref(),
    [] = subtract([{Ref,1}], [{Ref,1}]),
    [] = subtract([[Ref,1]], [[Ref,1]]),
    [] = subtract([#{Ref => [1]}], [#{Ref => [1]}]),
    [] = subtract([binary(42)], [binary(42)]),
    [] = subtract([big(Ref)], [big(Ref)]),
    %% Deliberately colliding flat-tuple fingerprints must not remove a value.
    Collisions = lists:duplicate(16, {1,128}),
    Collisions = subtract(Collisions, lists:duplicate(8, {0,0})),
    %% Mixed candidates and removals retain exact equality and multiplicity.
    [{tag,2}, nested] = subtract([{tag,1},{tag,2},nested,{tag,1}],
        [{tag,1},missing,missing,missing,missing,missing,missing,{tag,1}]),
    %% NERA-67: the shared exact comparator follows OTP 27+ signed-zero equality.
    [{tag,0.0}] = subtract([#{k => [1]}, {tag,0.0}],
        [#{k => [1]}, {tag,-0.0}, {x,1}, {x,2}, {x,3}, {x,4}, {x,5}, {x,6}]),
    [1] = subtract([1,1.0], [1.0]),
    [{Ref,1}] = subtract([{Ref,1}], [{Ref,1.0}]),
    caught = try [1|bad] ++ [2], error(continued_after_append)
             catch error:badarg -> caught end,
    caught = try [1|bad] -- [], error(continued_after_subtract)
             catch error:badarg -> caught end,
    {Pid, Monitor} = spawn_monitor(fun() -> append([1|bad], []), error(continued_after_append) end),
    receive {'DOWN', Monitor, process, Pid, {badarg, _Stack}} -> ok
    after 2000 -> error(missing_list_error) end,
    io:format("list_operator_contracts_ok~n"),
    ok.
append(A,B) -> A ++ B.
subtract(A,B) -> A -- B.
binary(N) -> <<N, 1, 2>>.
big(_) -> 1152921504606846976 + 1.
bad(F) -> {'EXIT', {badarg, _}} = (catch F()), ok.
