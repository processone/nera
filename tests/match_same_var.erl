%% Adapted from OTP compiler/test/match_SUITE.erl (Apache 2.0)
%% Tests repeated-variable patterns (equality constraints) across
%% tuples, lists and mixed structures, plus their failure modes.
-module(match_same_var).
-export([test/0]).

test() ->
    %% {Same, Same} via case
    same = pair_case({7, 7}),
    diff = pair_case({7, 8}),
    same = pair_case({hello, hello}),
    diff = pair_case({hello, world}),

    %% {Same, Same} via function head
    same = pair_fun(1, 1),
    diff = pair_fun(1, 2),

    %% Repeated var across three-element list
    yes = triple_list([7, 7, 7]),
    no = triple_list([7, 7, 8]),
    no = triple_list([1, 2, 3]),

    %% Repeated var across mixed tuple+list structure: {X, [X|_]}
    ok = mixed({a, [a, b, c]}),
    ok = mixed({42, [42]}),
    nope = mixed({a, [b, c]}),
    nope = mixed({1, []}),

    %% Repeated var with badmatch caught
    ok = try
             {X, X} = id({1, 2}),
             should_not_reach
         catch
             error:{badmatch, {1, 2}} -> ok
         end,

    %% Repeated var in list, badmatch caught
    ok = try
             [Y, Y] = id([7, 8]),
             should_not_reach
         catch
             error:{badmatch, [7, 8]} -> ok
         end,

    io:format("ok~n"),
    ok.

pair_case(T) ->
    case T of
        {X, X} -> same;
        _ -> diff
    end.

pair_fun(A, A) -> same;
pair_fun(_, _) -> diff.

triple_list([X, X, X]) -> yes;
triple_list(_) -> no.

mixed({X, [X | _]}) -> ok;
mixed(_) -> nope.

id(I) -> I.
