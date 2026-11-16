%% EXPECT: lists_keysort_ok
%% Run unchanged with Nera and BEAM. Equal keys must keep input order,
%% including integer/float keys that compare equal but are not identical.
-module(lists_keysort).
-export([test/0]).

test() ->
    check(1, [], []),
    check(2, [{only, key}], [{only, key}]),
    check(1, [{b, 1}, {a, 2}], [{a, 2}, {b, 1}]),
    check(2, [{z, 2}, {y, 1}, {x, 2}, {w, 1}],
             [{y, 1}, {w, 1}, {z, 2}, {x, 2}]),
    check(3, [{z, payload, 2}, {y, payload, 1}, {x, payload, 2}],
             [{y, payload, 1}, {z, payload, 2}, {x, payload, 2}]),
    %% Odd and even splits, with payloads deliberately in descending order.
    check(1, [{a, 5}, {a, 4}, {a, 3}, {a, 2}, {a, 1}],
             [{a, 5}, {a, 4}, {a, 3}, {a, 2}, {a, 1}]),
    check(1, [{b, 6}, {a, 5}, {b, 4}, {a, 3}, {b, 2}, {a, 1}],
             [{a, 5}, {a, 3}, {a, 1}, {b, 6}, {b, 4}, {b, 2}]),
    check(1, [{2, z}, {1.0, y}, {1, x}, {0, w}, {1.0, v}, {1, u}],
             [{0, w}, {1.0, y}, {1, x}, {1.0, v}, {1, u}, {2, z}]),
    check(1, [{[1.0], z}, {[1], y}, {[0], x}],
             [{[0], x}, {[1.0], z}, {[1], y}]),
    check(1, [{<<"b">>, bin}, {[a], list}, {[], nil}, {#{}, map},
              {{a}, tuple}, {zebra, atom_z}, {apple, atom_a}, {2, number}],
             [{2, number}, {apple, atom_a}, {zebra, atom_z}, {{a}, tuple},
              {#{}, map}, {[], nil}, {[a], list}, {<<"b">>, bin}]),
    %% Tuple size and non-key fields must not participate in the ordering.
    check(2, [{z, b}, {y, a, extra}, {x, a}],
             [{y, a, extra}, {x, a}, {z, b}]),
    Sorted = [{1, a}, {2, b}, {3, c}, {4, d}, {5, e}],
    check(1, Sorted, Sorted),
    check(1, lists:reverse(Sorted), Sorted),
    %% Independent bucket oracle: no sorting helper supplies expectations.
    Long = [{I rem 7, I} || I <- lists:seq(2000, 1, -1)],
    Expected = [{K, I} || K <- lists:seq(0, 6), {Key, I} <- Long, Key == K],
    check(1, Long, Expected),
    exhaustive(0),
    invalid_positions([0, -1, 1.0, bad]),
    badarg = failure(2, [{a}, {b}]),
    badarg = failure(1, [not_a_tuple, {a}]),
    io:format("lists_keysort_ok~n"),
    ok.

check(N, Input0, Expected) ->
    Input = id(Input0),
    true = Expected =:= lists:keysort(N, Input),
    true = Expected =:= apply(lists, keysort, [N, Input]),
    F = fun lists:keysort/2,
    true = Expected =:= F(N, Input),
    Module = id(lists), Function = id(keysort),
    true = Expected =:= Module:Function(N, Input),
    ok.

%% Every list of keys 0..2 of lengths 0..5, tagged by original position.
exhaustive(6) -> ok;
exhaustive(N) ->
    lists:foreach(fun(Keys) ->
        Input = lists:zip(Keys, lists:seq(1, N)),
        Expected = [{K, I} || K <- [0, 1, 2], {Key, I} <- Input, Key == K],
        true = Expected =:= lists:keysort(1, Input)
    end, key_lists(N)),
    exhaustive(N + 1).

key_lists(0) -> [[]];
key_lists(N) -> [[K|Rest] || K <- [0, 1, 2], Rest <- key_lists(N - 1)].

invalid_positions([]) -> ok;
invalid_positions([N|Rest]) ->
    function_clause = failure(N, []),
    function_clause = failure(N, [{a}]),
    invalid_positions(Rest).

failure(N, List) ->
    try lists:keysort(id(N), id(List)) of
        _ -> unexpected_success
    catch error:Reason -> Reason
    end.

id(X) -> X.
