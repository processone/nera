#!/usr/bin/env escript
%%! +S 1
%% NERA-92: repeat the preprocessed production-source name audit, not a runtime test.
main([Deps, Catalogue]) ->
    {ok, Text} = file:read_file(Catalogue),
    {match, Rows} = re:run(Text, <<"\\(\"([^\"]+)\", ([0-9]+)\\)">>,
                           [global, {capture, [1,2], binary}]),
    Known = ordsets:from_list([{binary_to_atom(F), binary_to_integer(A)} || [F,A] <- Rows]),
    Files = lists:sort(filelib:wildcard(filename:join([Deps,"*","src","*.erl"]))),
    Results = [audit(File, Deps, Known) || File <- Files],
    Used = ordsets:union([U || {_,U,_} <- Results]),
    Missing = ordsets:union([M || {_,_,M} <- Results]),
    io:format("Production modules: ~B; distinct OTP auto-imports used: ~B~n", [length(Files),length(Used)]),
    io:format("Names absent from Nera catalogue: ~p~n", [Missing]),
    case Missing of [] -> ok; _ -> halt(1) end;
main(_) -> io:format("Usage: audit-auto-imports.escript DEPS_ROOT OTP_CATALOGUE~n"), halt(2).

audit(File, Deps, Known) ->
    App = filename:dirname(filename:dirname(File)),
    {ok, Forms} = epp:parse_file(File, [Deps,filename:join(App,"include")], []),
    [] = [E || {error,E} <- Forms],
    Locals = [{F,A} || {function,_,F,A,_} <- Forms],
    Imports = lists:append([Fs || {attribute,_,import,{_,Fs}} <- Forms]),
    Options = lists:flatten([O || {attribute,_,compile,O} <- Forms]),
    Suppressed = lists:flatten([Fs || {no_auto_import,Fs} <- Options]),
    Calls = lists:append([calls(Form) || Form={function,_,_,_,_} <- Forms]),
    Used = ordsets:from_list([FA || FA={F,A} <- Calls, erl_internal:bif(F,A),
        not lists:member(FA,Locals), not lists:member(FA,Imports),
        not lists:member(no_auto_import,Options), not lists:member(FA,Suppressed)]),
    Missing = ordsets:subtract(Used,Known),
    io:format("~s: ~B names; absent ~p~n", [filename:basename(File),length(Used),Missing]),
    {File,Used,Missing}.

calls({call,_,{atom,_,F},Args}) -> [{F,length(Args)} | calls(Args)];
calls({'fun',_,{function,F,A}}) -> [{F,A}];
calls(T) when is_tuple(T) -> calls(tuple_to_list(T));
calls(L) when is_list(L) -> lists:append([calls(E) || E <- L]);
calls(_) -> [].
