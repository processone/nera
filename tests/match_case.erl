%% Adapted from OTP compiler/test/match_SUITE.erl (Apache 2.0)
%% Tests case expressions with various pattern types.
-module(match_case).
-export([test/0]).

test() ->
    %% Simple case
    one = num_word(1),
    two = num_word(2),
    three = num_word(3),
    other = num_word(42),

    %% Case with guards
    pos = case_guard(5),
    neg = case_guard(-3),
    zero = case_guard(0),

    %% Case with tuple patterns
    {ok, hello} = case_tuple({greeting, hello}),
    {ok, 42} = case_tuple({number, 42}),
    error = case_tuple(unknown),

    %% Nested case
    big_positive = nested_case(100),
    small_positive = nested_case(3),
    negative = nested_case(-5),
    zero = nested_case(0),

    %% Case returning complex values
    [1, 2, 3] = case_complex(list),
    {1, 2, 3} = case_complex(tuple),
    42 = case_complex(number),

    %% Case with multiple patterns sharing a body (not supported in standard
    %% Erlang case, but multiple function clauses work)
    vowel = classify_letter(a),
    vowel = classify_letter(e),
    vowel = classify_letter(i),
    consonant = classify_letter(b),
    consonant = classify_letter(c),

    %% Case with list patterns
    {first, 1} = case_list([1, 2, 3]),
    empty = case_list([]),
    {singleton, a} = case_list([a]),

    io:format("ok~n"),
    ok.

num_word(X) ->
    case X of
        1 -> one;
        2 -> two;
        3 -> three;
        _ -> other
    end.

case_guard(X) ->
    case X of
        X when X > 0 -> pos;
        X when X < 0 -> neg;
        0 -> zero
    end.

case_tuple(T) ->
    case T of
        {greeting, Name} -> {ok, Name};
        {number, N} -> {ok, N};
        _ -> error
    end.

nested_case(X) ->
    case X of
        0 -> zero;
        X when X > 0 ->
            case X > 10 of
                true -> big_positive;
                false -> small_positive
            end;
        _ -> negative
    end.

case_complex(What) ->
    case What of
        list -> [1, 2, 3];
        tuple -> {1, 2, 3};
        number -> 42
    end.

classify_letter(a) -> vowel;
classify_letter(e) -> vowel;
classify_letter(i) -> vowel;
classify_letter(o) -> vowel;
classify_letter(u) -> vowel;
classify_letter(_) -> consonant.

case_list(L) ->
    case L of
        [] -> empty;
        [X] -> {singleton, X};
        [H | _] -> {first, H}
    end.
