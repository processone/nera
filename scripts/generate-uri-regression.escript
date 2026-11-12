#!/usr/bin/env escript
%% Regenerate project-owned URI expectations with the installed BEAM stdlib.
main([Output]) ->
    Root = filename:dirname(filename:dirname(escript:script_name())),
    CasesPath = filename:join([Root,"scripts","fixtures","uri_cases.erl"]),
    {ok,uri_cases,Binary} = compile:file(CasesPath,[binary]),
    {module,uri_cases} = code:load_binary(uri_cases,CasesPath,Binary),
    Cases = [{F,A,try apply(uri_string,F,A) of R -> {return,R} catch C:E -> {C,E} end}
             || {F,A} <- uri_cases:cases()],
    Source = ["-module(uri_string_regression).\n-export([test/0]).\ntest() -> run(",
        io_lib:format("~w",[Cases]),
        ").\nrun([]) -> ok;\nrun([{F,A,Expected}|T]) ->\n Actual = try apply(uri_string,F,A) of R -> {return,R} catch C:E -> {C,E} end,\n case Actual =:= Expected of true -> run(T); false -> error({uri_case,F,A,Expected,Actual}) end.\n"],
    ok = file:write_file(Output,unicode:characters_to_binary(Source)),
    io:format("~p URI cases generated~n",[length(Cases)]).
