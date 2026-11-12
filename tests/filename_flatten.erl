%% EXPECT: ok
%% filename:flatten/1 is exported, as on OTP 29. OTP's EUnit server calls it,
%% so native EUnit runs of Cowlib's upstream suites failed to compile while
%% Nera's filename module kept it private. Expected values were taken from BEAM.
-module(filename_flatten).
-export([test/0]).
test() ->
    "a/b" = filename:flatten("a/b"),
    <<"a/b">> = filename:flatten(<<"a/b">>),
    "foo" = filename:flatten(foo),
    "a/bcd" = filename:flatten([a, "/", [b, ["c"]], d]),
    "xy z" = filename:flatten(["x", [], ['y z']]),
    [] = filename:flatten([]),
    <<>> = filename:flatten(<<>>),
    "ab" = filename:flatten([$a | b]),
    [<<"a">>, $b] = filename:flatten([<<"a">>, "b"]),
    function_clause = failure(fun() -> filename:flatten(5) end),
    "foo" = apply(filename, flatten, [foo]),
    F = fun filename:flatten/1,
    "foo" = F(foo),
    io:format("ok~n").
failure(F) -> try F() of _ -> unexpected_return catch error:R -> R end.
