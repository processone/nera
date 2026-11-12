%% EXPECT: binary_match_scope_ok
%% Regression: binary:match/3 is missing, which stops Cowlib 2.22.0 from
%% building. The scope option restricts the search to a window of the subject,
%% negative lengths search backward from Start, offsets stay relative to the
%% whole subject, and unknown options or windows outside the subject raise
%% badarg through direct, apply and fun reference calls. Run this fixture on
%% native Nera and BEAM.
-module(binary_match_scope).
-export([test/0]).

test() ->
    S = id(<<"abcabcabc">>),
    {0, 3} = binary:match(S, <<"abc">>, []),
    {3, 3} = binary:match(S, <<"abc">>, [{scope, {1, 8}}]),
    {6, 3} = binary:match(S, <<"abc">>, [{scope, {4, 5}}]),
    nomatch = binary:match(S, <<"abc">>, [{scope, {1, 2}}]),
    {0, 3} = binary:match(S, <<"abc">>, [{scope, {0, 3}}]),
    {2, 1} = binary:match(S, [<<"c">>, <<"bc">>], [{scope, {2, 3}}]),
    {1, 2} = binary:match(S, [<<"c">>, <<"bc">>], [{scope, {1, 3}}]),
    %% A negative length searches the bytes before Start.
    {3, 3} = binary:match(S, <<"abc">>, [{scope, {6, -4}}]),
    nomatch = binary:match(S, <<"abc">>, [{scope, {2, -2}}]),
    {0, 3} = binary:match(S, <<"abc">>, [{scope, {9, -9}}]),
    %% Empty windows, at either end.
    nomatch = binary:match(S, <<"abc">>, [{scope, {0, 0}}]),
    nomatch = binary:match(S, <<"abc">>, [{scope, {9, 0}}]),
    %% The scope may hit a window inside a sub-binary.
    <<_:8, Sub/binary>> = S,
    {2, 3} = binary:match(Sub, <<"abc">>, [{scope, {1, 7}}]),
    %% Dynamic calls.
    {3, 3} = apply(binary, match, [S, <<"abc">>, [{scope, {1, 8}}]]),
    {3, 3} = (fun binary:match/3)(S, <<"abc">>, [{scope, {1, 8}}]),
    Mod = id(binary), Fun = id(match),
    {3, 3} = Mod:Fun(S, <<"abc">>, [{scope, {1, 8}}]),
    %% Inputs BEAM rejects.
    lists:foreach(fun({Subject, Pattern, Options}) ->
        badarg = reason(fun() -> binary:match(id(Subject), Pattern, Options) end),
        badarg = reason(fun() -> apply(binary, match, [Subject, Pattern, Options]) end)
    end, [{S, <<"abc">>, [{scope, {10, 1}}]},
          {S, <<"abc">>, [{scope, {-1, 3}}]},
          {S, <<"abc">>, [{scope, {0, 10}}]},
          {S, <<"abc">>, [{scope, {3, -4}}]},
          {S, <<"abc">>, [{scope, {a, 1}}]},
          {S, <<"abc">>, [{scope, 1}]},
          {S, <<"abc">>, [foo]},
          {S, <<"abc">>, [{foo, {0, 1}}]},
          {S, <<"abc">>, [{scope, {0, 1, 2}}]},
          {S, <<"abc">>, foo},
          {S, <<>>, []},
          {S, [], []},
          {S, foo, []},
          {foo, <<"a">>, []},
          {<<1:3>>, <<"a">>, []}]),
    io:format("binary_match_scope_ok~n"),
    ok.

reason(F) ->
    try F() of
        Value -> {returned, Value}
    catch
        error:Reason -> Reason
    end.

id(X) -> X.
