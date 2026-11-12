%% EXPECT: binary_rest_guard_sequence_ok
%% Regression: in a guard sequence G1; G2, when G1 raises before it reads a
%% deferred binary tail (Token in <<"Bearer ", Token/binary>>), G2 and the
%% clause body read the tail as BEAM does. Before the fix, G2 compared an
%% undefined value, so the clause was skipped. The same fixture runs on BEAM.
-module(binary_rest_guard_sequence).
-export([test/0]).

-define(SERVICE_TOKEN, <<"svc-7f3a">>).

test() ->
    %% Authorization header values.
    Service = id(<<"Bearer svc-7f3a">>),
    User = id(<<"Bearer usr-19c2">>),
    Guest = id(<<"Bearer guest">>),
    %% A session is a {Token, Name} tuple; on a session stored as a map,
    %% element/2 raises badarg, so the first guard test fails.
    Tuple = id({<<"usr-19c2">>, alice}),
    Map = id(#{token => <<"usr-19c2">>, name => alice}),

    %% if: G1 raises, then G2 accepts the service token.
    {{accepted, <<"svc-7f3a">>}, <<"svc-7f3a">>} = if_auth(Service, Map),
    {rejected, <<"guest">>} = if_auth(Guest, Map),
    {{accepted, <<"usr-19c2">>}, <<"usr-19c2">>} = if_auth(User, Tuple),
    {{accepted, <<"svc-7f3a">>}, <<"svc-7f3a">>} = if_auth(Service, Tuple),
    %% Two tests raise before the third reads the token.
    {accepted, <<"svc-7f3a">>} = if_auth_any(Service, Map),
    rejected = if_auth_any(Guest, Map),
    {accepted, <<"usr-19c2">>} = if_auth_any(User, Tuple),

    %% case: the same sequence in a clause guard.
    {{accepted, <<"svc-7f3a">>}, <<"svc-7f3a">>} = case_auth(Service, Map),
    {rejected, <<"guest">>} = case_auth(Guest, Map),
    {{accepted, <<"usr-19c2">>}, <<"usr-19c2">>} = case_auth(User, Tuple),

    %% A function head guard reads the tail its own pattern binds.
    {accepted, <<"svc-7f3a">>} = head_auth(Service, Map),
    rejected = head_auth(Guest, Map),
    {accepted, <<"usr-19c2">>} = head_auth(User, Tuple),

    %% A receive clause guard, with the session already queued.
    {accepted, <<"svc-7f3a">>} = receive_auth(Service, Map),
    rejected = receive_auth(Guest, Map),

    io:format("binary_rest_guard_sequence_ok~n"),
    ok.

if_auth(<<"Bearer ", Token/binary>>, Session) ->
    Result = if
        element(1, Session) =:= Token; Token =:= ?SERVICE_TOKEN -> {accepted, Token};
        true -> rejected
    end,
    {Result, Token}.

if_auth_any(<<"Bearer ", Token/binary>>, Session) ->
    if
        element(1, Session) =:= Token;
        element(2, Session) =:= Token;
        Token =:= ?SERVICE_TOKEN -> {accepted, Token};
        true -> rejected
    end.

case_auth(<<"Bearer ", Token/binary>>, Session) ->
    Result = case Session of
        S when element(1, S) =:= Token; Token =:= ?SERVICE_TOKEN -> {accepted, Token};
        _ -> rejected
    end,
    {Result, Token}.

head_auth(<<"Bearer ", Token/binary>>, Session)
        when element(1, Session) =:= Token; Token =:= ?SERVICE_TOKEN ->
    {accepted, Token};
head_auth(_, _) ->
    rejected.

receive_auth(<<"Bearer ", Token/binary>>, Session) ->
    self() ! {session, Session},
    receive
        {session, S} when element(1, S) =:= Token; Token =:= ?SERVICE_TOKEN ->
            {accepted, Token};
        {session, _} ->
            rejected
    end.

id(X) -> X.
