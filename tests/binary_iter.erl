-module(binary_iter).
-export([test/0]).

test() ->
    %% Basic count_bytes: recursive binary iteration
    0 = count_bytes(<<>>, 0),
    1 = count_bytes(<<42>>, 0),
    5 = count_bytes(<<1,2,3,4,5>>, 0),
    10 = count_bytes(<<1,2,3,4,5,6,7,8,9,10>>, 0),

    %% Sum of bytes
    0 = sum_bytes(<<>>),
    255 = sum_bytes(<<255>>),
    15 = sum_bytes(<<1,2,3,4,5>>),

    %% Collect bytes to list (match context must materialize on cons)
    [] = bytes_to_list(<<>>),
    [1,2,3] = bytes_to_list(<<1,2,3>>),

    %% Nested match: outer receives match ctx, inner materializes rest
    {3, <<4,5>>} = split_at_3(<<1,2,3,4,5>>),

    %% Match context through multiple function calls
    ok = verify_sequence(<<1,2,3,4,5>>, 1),

    ok.

count_bytes(<<>>, Acc) -> Acc;
count_bytes(<<_, Rest/binary>>, Acc) ->
    count_bytes(Rest, Acc + 1).

sum_bytes(Bin) -> sum_bytes(Bin, 0).

sum_bytes(<<>>, Acc) -> Acc;
sum_bytes(<<B, Rest/binary>>, Acc) ->
    sum_bytes(Rest, Acc + B).

bytes_to_list(<<>>) -> [];
bytes_to_list(<<B, Rest/binary>>) ->
    [B | bytes_to_list(Rest)].

split_at_3(Bin) -> split_at_3(Bin, 0).
split_at_3(Rest, 3) -> {3, Rest};
split_at_3(<<_, Rest/binary>>, N) ->
    split_at_3(Rest, N + 1).

verify_sequence(<<>>, _) -> ok;
verify_sequence(<<Expected, Rest/binary>>, Expected) ->
    verify_sequence(Rest, Expected + 1).
