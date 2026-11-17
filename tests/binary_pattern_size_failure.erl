%% EXPECT: binary pattern size failure passed
%% A size expression that raises rejects the pattern, not the process.
-module(binary_pattern_size_failure).
-export([test/0]).

test() ->
    lists:foreach(fun({N, B, Expected}) ->
        Expected = case_size(N, B),
        Expected = fun_size(N, B),
        Expected = try_size(N, B),
        Expected = try_of_size(N, B),
        Expected = catch_size(N, B)
    end, [{a, <<7>>, nomatch}, {1, <<7>>, {v, 7}},
          {0, <<>>, {v, 0}}, {1, <<>>, nomatch},
          {1, <<7, 8>>, nomatch}, {-1, <<7>>, nomatch},
          {1.0, <<7>>, nomatch}]),
    lists:foreach(fun({Op, Bad, Good, B, Expected}) ->
        nomatch = operator_size(Op, Bad, B),
        {v, Expected} = operator_size(Op, Good, B)
    end, [{add, a, 0, <<7>>, 7}, {divide, 0, 1, <<7>>, 7},
          {nested, a, {1}, <<7>>, 7}, {negate, a, -8, <<7>>, 7},
          {bitnot, a, -9, <<7>>, 7}, {bif, a, <<0>>, <<1:1>>, 1}]),
    lists:foreach(fun({Type, N, B}) ->
        nomatch = segment_size(Type, a, B),
        {v, N} = segment_size(Type, 1, B)
    end, [{binary, <<7>>, <<7>>}, {bitstring, <<7:3>>, <<7:3>>},
          {float, 7.0, <<7.0/float>>}]),
    nomatch = head_size(<<0, 7>>),
    {v, 7} = head_size(<<1, 7>>),
    {fallback, 7} = next_binary_clause(a, <<7>>),
    after_match = later_error(a, <<7>>),
    {badmatch, <<7>>} = match_size(a, <<7>>),
    {v, 7} = match_size(1, <<7>>),
    {unmatched, <<7>>} = maybe_size(a, <<7>>),
    {v, 7} = maybe_size(1, <<7>>),
    nomatch = receive_size(a),
    receive <<7>> -> ok after 0 -> error(message_was_consumed) end,
    {v, 7} = receive_size(1),
    receive fallback -> ok after 0 -> error(missing_fallback) end,
    [] = list_sizes(a, [<<7>>, <<8>>]),
    [7, 8] = list_sizes(1, [<<7>>, <<8>>]),
    [] = binary_sizes(<<0, 7>>),
    [7, 8] = binary_sizes(<<1, 7, 1, 8>>),
    io:format("binary pattern size failure passed~n"),
    ok.

case_size(N, B) ->
    case B of <<V:(N*8)>> -> {v, V}; _ -> nomatch end.

fun_size(N, B) ->
    F = fun(<<V:(N*8)>>) -> {v, V}; (_) -> nomatch end,
    F(B).

try_size(N, B) ->
    try
        Result = case B of <<V:(N*8)>> -> {v, V}; _ -> nomatch end,
        Result
    catch Class:Reason -> {unexpected, Class, Reason}
    end.

try_of_size(N, B) ->
    try id(B) of
        <<V:(N*8)>> -> {v, V};
        _ -> nomatch
    catch Class:Reason -> {unexpected, Class, Reason}
    end.

catch_size(N, B) ->
    try throw(B)
    catch
        throw:<<V:(N*8)>> -> {v, V};
        throw:_ -> nomatch
    end.

operator_size(add, N, B) ->
    case B of <<V:(N+8)>> -> {v, V}; _ -> nomatch end;
operator_size(divide, N, B) ->
    case B of <<V:(8 div N)>> -> {v, V}; _ -> nomatch end;
operator_size(nested, N, B) ->
    case B of <<V:((element(1, N)+0)*8)>> -> {v, V}; _ -> nomatch end;
operator_size(negate, N, B) ->
    case B of <<V:(-N)>> -> {v, V}; _ -> nomatch end;
operator_size(bitnot, N, B) ->
    case B of <<V:(bnot N)>> -> {v, V}; _ -> nomatch end;
operator_size(bif, N, B) ->
    case B of <<V:(byte_size(N))>> -> {v, V}; _ -> nomatch end.

segment_size(binary, N, B) ->
    case B of <<V:(N*1)/binary>> -> {v, V}; _ -> nomatch end;
segment_size(bitstring, N, B) ->
    case B of <<V:(N*3)/bitstring>> -> {v, V}; _ -> nomatch end;
segment_size(float, N, B) ->
    case B of <<V:(N*64)/float>> -> {v, V}; _ -> nomatch end.

head_size(<<N, V:(8 div N)>>) -> {v, V};
head_size(_) -> nomatch.

next_binary_clause(N, B) ->
    case B of
        <<V:(N*8)>> -> {v, V};
        <<V>> -> {fallback, V};
        _ -> nomatch
    end.

later_error(N, B) ->
    try
        nomatch = case B of <<_:(N*8)>> -> matched; _ -> nomatch end,
        error(after_match)
    catch error:Reason -> Reason
    end.

match_size(N, B) ->
    try
        <<V:(N*8)>> = B,
        {v, V}
    catch error:Reason -> Reason
    end.

maybe_size(N, B) ->
    maybe
        <<V:(N*8)>> ?= B,
        {v, V}
    else
        Other -> {unmatched, Other}
    end.

receive_size(N) ->
    self() ! <<7>>,
    self() ! fallback,
    receive
        <<V:(N*8)>> -> {v, V};
        fallback -> nomatch
    after 1000 -> error(missing_message)
    end.

list_sizes(N, Bs) -> [V || <<V:(N*8)>> <- Bs].
binary_sizes(B) -> [V || <<N, V:(8 div N)>> <= B].
id(X) -> X.
