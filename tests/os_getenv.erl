-module(os_getenv).
-export([test/0]).
test() ->
    Path = os:getenv("PATH"),
    true = is_list(Path),
    true = length(Path) > 0,
    Path = apply(os, getenv, ["PATH"]),
    Path = os:getenv(["PA", [84, 72]]),
    false = os:getenv([[[]]]),
    false = os:getenv("NERA_TEST_UNSET_ENVIRONMENT_7F956B3658F5"),
    false = os:getenv([]),
    lists:foreach(fun(Name) ->
        try os:getenv(Name) of _ -> error(expected_badarg)
        catch error:badarg -> ok end
    end, [atom, <<"PATH">>, [0], "A=B", [-1], [1114112], [4294967361], [65|bad], ["PA", <<"TH">>]]),
    ok.
