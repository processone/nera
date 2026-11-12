%% Test: multi-clause functions with binary prefix patterns and Rest/binary.
%%
%% Regression test: the pattern compiler used to share variable scopes
%% across clauses, causing the VarId for Rest/binary in one clause to
%% be overwritten by a later clause's binding.  This led to the body
%% of the first clause referencing the wrong VarId (from another
%% clause), producing garbage or crashing at codegen.
%%
-module(multi_clause_binary_rest).
-export([test/0]).

test() ->
    %% Basic multi-clause binary prefix dispatch
    {set, <<"hello">>} = dispatch(<<"set hello">>),
    {get, <<"world">>} = dispatch(<<"get world">>),
    {del, <<"key1">>}  = dispatch(<<"del key1">>),
    unknown = dispatch(<<"quit">>),

    %% Verify that Rest is a real binary in each clause
    true = check_types(<<"set abc">>),
    true = check_types(<<"get xyz">>),

    %% Empty rest after prefix
    {set, <<>>} = dispatch(<<"set ">>),
    {get, <<>>} = dispatch(<<"get ">>),

    %% Longer rest
    {set, <<"a long value with spaces">>} = dispatch(<<"set a long value with spaces">>),

    %% Multi-clause with second argument (like onecached's handle_command/2)
    {set, <<"k">>, ctx1} = dispatch2(<<"set k">>, ctx1),
    {get, <<"k">>, ctx2} = dispatch2(<<"get k">>, ctx2),
    {other, <<"quit">>, ctx3} = dispatch2(<<"quit">>, ctx3),

    %% Three-clause binary prefix with different prefix lengths
    {short, <<"x">>} = prefix_len(<<"ab x">>),
    {long, <<"y">>}  = prefix_len(<<"abcde y">>),
    nomatch = prefix_len(<<"zzz">>),

    ok.

dispatch(<<"set ", Rest/binary>>) -> {set, Rest};
dispatch(<<"get ", Rest/binary>>) -> {get, Rest};
dispatch(<<"del ", Rest/binary>>) -> {del, Rest};
dispatch(_Other) -> unknown.

check_types(<<"set ", R/binary>>) ->
    is_binary(R);
check_types(<<"get ", R/binary>>) ->
    is_binary(R);
check_types(_) ->
    false.

dispatch2(<<"set ", R/binary>>, Ctx) -> {set, R, Ctx};
dispatch2(<<"get ", R/binary>>, Ctx) -> {get, R, Ctx};
dispatch2(Other, Ctx) -> {other, Other, Ctx}.

prefix_len(<<"ab ", R/binary>>) -> {short, R};
prefix_len(<<"abcde ", R/binary>>) -> {long, R};
prefix_len(_) -> nomatch.
