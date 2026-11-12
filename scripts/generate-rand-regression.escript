#!/usr/bin/env escript
%% Regenerate expected/0 in tests/rand_regression.erl with the installed
%% BEAM stdlib's rand module. Run it with OTP 29:
%%   escript scripts/generate-rand-regression.escript tests/rand_regression.erl
main([Path]) ->
    "29" = erlang:system_info(otp_release),
    {ok, Source} = file:read_file(Path),
    {ok, rand_regression, Binary} = compile:file(Path, [binary, report]),
    {module, rand_regression} = code:load_binary(rand_regression, Path, Binary),
    Observed = rand_regression:observe(),
    Lines = [io_lib:format("     ~w", [Entry]) || Entry <- Observed],
    Generated = ["%% BEGIN GENERATED expected/0\nexpected() ->\n    [\n",
                 lists:join(",\n", Lines),
                 "\n    ].\n%% END GENERATED expected/0\n"],
    [Before, Rest] = binary:split(Source, <<"%% BEGIN GENERATED expected/0\n">>),
    [_, After] = binary:split(Rest, <<"%% END GENERATED expected/0\n">>),
    ok = file:write_file(Path, [Before, Generated, After]),
    io:format("~p rand observations generated~n", [length(Observed)]).
