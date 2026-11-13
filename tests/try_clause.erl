%% EXPECT: try_clause_ok
%% Unmatched try-of results raise error:{try_clause, Value}, outside the
%% try's own catch clauses, after running any after body once. Run this
%% same fixture on native Nera and BEAM.
-module(try_clause).
-export([test/0]).

test() ->
    Values = [unexpected, 42, 1.5, [], [1, two | tail],
              {accepted, 0}, {accepted, not_integer},
              {reply, error, <<"payload">>}, <<"bytes">>,
              #{status => rejected, detail => [1, 2]}, 1 bsl 100],
    lists:foreach(fun(Value) ->
        {error, {try_clause, Value}} = outcome(fun() -> unmatched(Value) end),
        {error, {try_clause, Value}} = outcome(fun() -> guarded(Value) end),
        {error, {try_clause, Value}} = outcome(fun() -> with_after(Value) end),
        [cleaned] = drain(),
        {error, {try_clause, Value}} = outcome(fun() -> with_catch_after(Value) end),
        [cleaned] = drain()
    end, Values),

    %% An enclosing handler in the same function sees the tuple too.
    Value = id({unmatched, <<"outer">>}),
    {try_clause, Value} = try
        try id(Value) of
            accepted -> wrong
        catch
            _:_ -> wrong_local_handler
        end
    catch
        error:Reason -> Reason
    end,

    %% Matching clauses still return their result; body exceptions still
    %% reach this try's catch, unlike failures in the of clauses.
    matched = unmatched(accepted),
    matched = guarded({accepted, 7}),
    matched = with_after(accepted),
    [cleaned] = drain(),
    caught_body = try error(body_failure) of
        _ -> wrong
    catch
        error:body_failure -> caught_body
    end,
    {error, of_failure} = outcome(fun() ->
        try id(accepted) of
            accepted -> error(of_failure)
        catch
            _:_ -> wrong_local_handler
        end
    end),
    [] = drain(),
    io:format("try_clause_ok~n"),
    ok.

unmatched(Value) ->
    try id(Value) of
        accepted -> matched
    catch
        _:_ -> wrong_local_handler
    end.

guarded(Value) ->
    try id(Value) of
        {accepted, N} when is_integer(N), N > 0 -> matched;
        _ when Value =:= accepted -> matched
    catch
        _:_ -> wrong_local_handler
    end.

with_after(Value) ->
    try id(Value) of
        accepted -> matched
    after
        self() ! cleaned
    end.

with_catch_after(Value) ->
    try id(Value) of
        accepted -> matched
    catch
        _:_ -> wrong_local_handler
    after
        self() ! cleaned
    end.

outcome(F) ->
    try {ok, F()} catch Class:Reason -> {Class, Reason} end.

drain() -> receive Message -> [Message | drain()] after 0 -> [] end.

id(Value) -> Value.
