%% Tests for maybe...end expression (EEP-49, OTP 25+).
-module(maybe_basic).
-export([test/0]).

test() ->
    %% Basic ?= success path
    42 = basic_success({ok, 42}),

    %% ?= failure with else clause
    {error, reason} = with_else({error, reason}),

    %% ?= success with else clause (else not taken)
    42 = with_else({ok, 42}),

    %% Multiple ?= in sequence — all succeed
    3 = multi_match({ok, 1}, {ok, 2}),

    %% Multiple ?= — first fails
    {error, bad} = multi_match({error, bad}, {ok, 2}),

    %% Multiple ?= — second fails
    {error, bad2} = multi_match({ok, 1}, {error, bad2}),

    %% Plain maybe block (no ?=), acts like begin...end
    3 = plain_block(),

    %% Nested maybe expressions
    ok = nested_maybe({ok, {ok, inner}}),

    io:format("ok~n"),
    ok.

%% Basic: ?= success returns the bound value
basic_success(X) ->
    maybe
        {ok, V} ?= X,
        V
    end.

%% With else: pattern match the failed value
with_else(X) ->
    maybe
        {ok, V} ?= X,
        V
    else
        {error, R} -> {error, R}
    end.

%% Multiple ?= in sequence
multi_match(X, Y) ->
    maybe
        {ok, A} ?= X,
        {ok, B} ?= Y,
        A + B
    else
        {error, R} -> {error, R}
    end.

%% Plain block — no ?=, just expressions
plain_block() ->
    maybe
        1 + 2
    end.

%% Nested maybe
nested_maybe(X) ->
    maybe
        {ok, Inner} ?= X,
        maybe
            {ok, _V} ?= Inner,
            ok
        else
            _ -> inner_fail
        end
    else
        _ -> outer_fail
    end.
