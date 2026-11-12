%% Adapted from OTP compiler/test/bs_match_SUITE.erl (Apache 2.0)
%% Tests binary pattern matching and construction with dynamic (variable) sizes.
-module(binary_dynamic_size).
-export([test/0]).

test() ->
    %% --- Construction with variable sizes ---

    %% Integer with variable bit size
    <<42:16>> = build_int(42, 16),
    <<255:8>> = build_int(255, 8),
    <<1:32>> = build_int(1, 32),

    %% Binary segment with variable byte count
    <<1, 2, 3>> = build_bin_prefix(<<1, 2, 3, 4, 5>>, 3),
    <<>> = build_bin_prefix(<<1, 2, 3>>, 0),
    <<1, 2, 3, 4, 5>> = build_bin_prefix(<<1, 2, 3, 4, 5>>, 5),

    %% --- Pattern matching with variable sizes ---

    %% Extract N bytes from front
    {<<1, 2>>, <<3, 4, 5>>} = split_at(<<1, 2, 3, 4, 5>>, 2),
    {<<1>>, <<2, 3, 4, 5>>} = split_at(<<1, 2, 3, 4, 5>>, 1),
    {<<>>, <<1, 2, 3>>} = split_at(<<1, 2, 3>>, 0),
    {<<1, 2, 3>>, <<>>} = split_at(<<1, 2, 3>>, 3),

    %% Extract N-bit integer
    42 = extract_int(<<42>>, 8),
    258 = extract_int(<<1, 2>>, 16),

    %% Skip N bytes, take rest
    <<3, 4, 5>> = skip_bytes(<<1, 2, 3, 4, 5>>, 2),
    <<1, 2, 3>> = skip_bytes(<<1, 2, 3>>, 0),

    %% Variable size from earlier segment (size in header)
    {hello, <<1, 2, 3, 4, 5>>} = parse_tlv(<<5, 1, 2, 3, 4, 5>>),
    {hello, <<42>>} = parse_tlv(<<1, 42>>),
    {hello, <<>>} = parse_tlv(<<0>>),

    %% Multi-field with variable sizes
    {<<1, 2>>, <<3, 4, 5>>} = split_two(<<2, 3, 1, 2, 3, 4, 5>>, id(0)),

    %% Dynamic size in case expression
    ok = case_dynamic_size(<<1, 2, 3, 4>>, 2),

    %% Function head pattern with computed size
    {<<10, 20>>, <<30>>} = head_pattern(<<10, 20, 30>>, 2),

    io:format("ok~n"),
    ok.

%% Construction helpers
build_int(Value, Size) ->
    <<Value:Size>>.

build_bin_prefix(Bin, Len) ->
    <<Prefix:Len/binary, _/binary>> = Bin,
    Prefix.

%% Pattern matching helpers
split_at(Bin, N) ->
    <<Front:N/binary, Rest/binary>> = Bin,
    {Front, Rest}.

extract_int(Bin, Bits) ->
    <<Value:Bits, _/binary>> = Bin,
    Value.

skip_bytes(Bin, N) ->
    <<_:N/binary, Rest/binary>> = Bin,
    Rest.

%% TLV: first byte is length, then that many bytes of payload
parse_tlv(<<Len, Payload:Len/binary>>) ->
    {hello, Payload}.

%% Two length-prefixed fields
split_two(<<L1, L2, D1:L1/binary, D2:L2/binary>>, _Extra) ->
    {D1, D2}.

%% Dynamic size in case
case_dynamic_size(Bin, N) ->
    case Bin of
        <<_:N/binary, _:N/binary>> -> ok;
        _ -> error
    end.

%% Function head with variable size
head_pattern(Bin, N) ->
    <<Front:N/binary, Rest/binary>> = Bin,
    {Front, Rest}.

id(I) -> I.
