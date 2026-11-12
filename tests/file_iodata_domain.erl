-module(file_iodata_domain).
-export([test/0]).
test() ->
    Path = "/tmp/nera_file_iodata_domain_test.txt",
    ok = file:write_file(Path, <<"unchanged">>),
    lists:foreach(fun(Data) ->
        {'EXIT', {badarg, _}} = (catch file:write_file(Path, Data)),
        {ok, <<"unchanged">>} = file:read_file(Path)
    end, [[256], [-1], [65, invalid], [65 | 7], 65, invalid, [<<1:1>>]]),
    ok = file:write_file(Path, [0, [255, <<"AB">>, []], [67 | <<"D">>]]),
    {ok, <<0,255,65,66,67,68>>} = file:read_file(Path),
    ok = file:write_file(Path, []), {ok, <<>>} = file:read_file(Path), ok.
