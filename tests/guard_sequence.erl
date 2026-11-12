%% Adapted from OTP compiler/test/guard_SUITE.erl (Apache 2.0)
%% Tests guard sequences (conjunction with comma, disjunction with semicolon).
-module(guard_sequence).
-export([test/0]).

test() ->
    %% Guard conjunction (comma = AND)
    ok = conj(5, 10),
    fail = conj(15, 10),
    fail = conj(-1, 10),

    %% Guard disjunction (semicolon = OR)
    ok = disj(0),
    ok = disj(100),
    fail = disj(50),
    fail = disj(49),
    fail = disj(51),

    %% Multiple guard clauses
    small = classify(3),
    medium = classify(50),
    large = classify(500),

    %% Guard with complex boolean combinations
    yes = check(true, true),
    no = check(true, false),
    no = check(false, true),
    no = check(false, false),

    %% Guard with andalso/orelse in guard
    ok = guard_andalso(5),
    fail = guard_andalso(15),
    fail = guard_andalso(-1),

    ok = guard_orelse(0),
    ok = guard_orelse(100),
    fail = guard_orelse(50),

    %% Nested guard disjunction
    ok = multi_guard(1, a),
    ok = multi_guard(2, b),
    ok = multi_guard(100, c),
    fail = multi_guard(50, c),

    io:format("ok~n"),
    ok.

%% Conjunction: both conditions must be true
conj(X, Max) when X >= 0, X =< Max -> ok;
conj(_, _) -> fail.

%% Disjunction: either condition suffices
disj(X) when X == 0; X >= 100 -> ok;
disj(_) -> fail.

classify(X) when X >= 0, X < 10 -> small;
classify(X) when X >= 10, X < 100 -> medium;
classify(X) when X >= 100 -> large.

check(X, Y) when X == true, Y == true -> yes;
check(_, _) -> no.

guard_andalso(X) when X >= 0 andalso X =< 10 -> ok;
guard_andalso(_) -> fail.

guard_orelse(X) when X == 0 orelse X >= 100 -> ok;
guard_orelse(_) -> fail.

multi_guard(X, _) when X == 1; X == 2 -> ok;
multi_guard(X, _) when X >= 100 -> ok;
multi_guard(_, _) -> fail.
