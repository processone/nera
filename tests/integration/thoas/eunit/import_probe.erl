%% Project-owned integration probe; uses unmodified external Thoas sources.
-module(import_probe).
-compile({no_auto_import, [float/1]}).
-import(thoas_encode, [true/0, false/0, null/0, float/1, string/1]).
-export([test/0]).
test() ->
    <<"true">> = true(),
    <<"false">> = false(),
    <<"null">> = null(),
    <<"1.25">> = float(1.25),
    F = fun thoas_encode:float/1,
    <<"-0.0">> = F(-0.0),
    <<"\"hello\"">> = iolist_to_binary(string(<<"hello">>)),
    io:format("THOAS_IMPORTS_OK~n").
