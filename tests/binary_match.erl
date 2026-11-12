%% Adapted from OTP compiler/test/bs_match_SUITE.erl (Apache 2.0)
%% Tests binary pattern matching.
-module(binary_match).
-export([test/0]).

test() ->
    %% Simple byte extraction
    {1, 2, 3} = extract3(<<1, 2, 3>>),

    %% Head and tail
    {1, <<2, 3>>} = head_tail(<<1, 2, 3>>),

    %% Size-based extraction
    {258, <<3, 4>>} = extract16(<<1, 2, 3, 4>>),

    %% Match empty binary
    empty = classify_bin(<<>>),
    nonempty = classify_bin(<<1>>),
    nonempty = classify_bin(<<1, 2, 3>>),

    %% Binary in case
    {ok, 42} = parse_packet(<<1, 42>>),
    {error, 99} = parse_packet(<<2, 99>>),

    %% Rest pattern
    <<3, 4, 5>> = skip_first_two(<<1, 2, 3, 4, 5>>),

    %% Matching with guards
    big = classify_byte(<<200>>),
    small = classify_byte(<<50>>),

    %% Multiple patterns
    ok = multi_match(<<1, 2, 3, 4>>),

    io:format("ok~n"),
    ok.

extract3(<<A, B, C>>) -> {A, B, C}.

head_tail(<<H, Rest/binary>>) -> {H, Rest}.

extract16(<<V:16, Rest/binary>>) -> {V, Rest}.

classify_bin(<<>>) -> empty;
classify_bin(_) -> nonempty.

parse_packet(Bin) ->
    case Bin of
        <<1, Value>> -> {ok, Value};
        <<2, Code>> -> {error, Code};
        _ -> unknown
    end.

skip_first_two(<<_, _, Rest/binary>>) -> Rest.

classify_byte(<<B>>) when B >= 128 -> big;
classify_byte(<<_>>) -> small.

multi_match(<<A, B, C, D>>) ->
    10 = A + B + C + D,
    ok.
