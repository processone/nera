%% Tests: IPv4 header parsing with sub-byte fields, construct and round-trip.
-module(ip_packet_parse).
-export([test/0]).

test() ->
    %% Construct a minimal IPv4 header (20 bytes, no options)
    Packet = make_ipv4(
        4,          %% Version
        5,          %% IHL (5 = 20 bytes)
        0,          %% DSCP
        0,          %% ECN
        40,         %% Total length
        1234,       %% Identification
        0,          %% Flags
        0,          %% Fragment offset
        64,         %% TTL
        6,          %% Protocol (TCP)
        0,          %% Checksum (placeholder)
        {192,168,1,1},   %% Source IP
        {10,0,0,1}       %% Dest IP
    ),

    20 = byte_size(Packet),

    %% Parse it back
    {ok, Fields} = parse_ipv4(Packet),
    4 = maps:get(version, Fields),
    5 = maps:get(ihl, Fields),
    40 = maps:get(total_length, Fields),
    1234 = maps:get(identification, Fields),
    64 = maps:get(ttl, Fields),
    6 = maps:get(protocol, Fields),
    {192,168,1,1} = maps:get(src_ip, Fields),
    {10,0,0,1} = maps:get(dst_ip, Fields),

    %% Sub-byte round-trip: 4-bit fields
    <<4:4, 5:4>> = <<69>>,   %% 4*16 + 5 = 69
    <<V:4, I:4>> = <<69>>,
    4 = V,
    5 = I,

    %% More sub-byte patterns
    ok = nibble_test(),

    %% Flags and fragment offset (3 + 13 bits)
    ok = flags_test(),

    io:format("ok~n"),
    ok.

make_ipv4(Version, IHL, DSCP, ECN, TotalLen, Id, Flags, FragOff,
          TTL, Proto, Checksum, {S1,S2,S3,S4}, {D1,D2,D3,D4}) ->
    <<Version:4, IHL:4, DSCP:6, ECN:2,
      TotalLen:16, Id:16,
      Flags:3, FragOff:13,
      TTL:8, Proto:8, Checksum:16,
      S1:8, S2:8, S3:8, S4:8,
      D1:8, D2:8, D3:8, D4:8>>.

parse_ipv4(<<Version:4, IHL:4, _DSCP:6, _ECN:2,
             TotalLen:16, Id:16,
             _Flags:3, _FragOff:13,
             TTL:8, Proto:8, _Checksum:16,
             S1:8, S2:8, S3:8, S4:8,
             D1:8, D2:8, D3:8, D4:8,
             _Rest/binary>>) ->
    {ok, #{version => Version,
           ihl => IHL,
           total_length => TotalLen,
           identification => Id,
           ttl => TTL,
           protocol => Proto,
           src_ip => {S1, S2, S3, S4},
           dst_ip => {D1, D2, D3, D4}}};
parse_ipv4(_) ->
    {error, invalid}.

nibble_test() ->
    %% Pack and unpack nibbles
    Packed = pack_nibbles([1, 2, 3, 4, 5, 6]),
    3 = byte_size(Packed),
    [1, 2, 3, 4, 5, 6] = unpack_nibbles(Packed),
    ok.

pack_nibbles([]) -> <<>>;
pack_nibbles([H, L | Rest]) ->
    Byte = <<H:4, L:4>>,
    RestBin = pack_nibbles(Rest),
    <<Byte/binary, RestBin/binary>>.

unpack_nibbles(<<>>) -> [];
unpack_nibbles(<<H:4, L:4, Rest/binary>>) ->
    [H, L | unpack_nibbles(Rest)].

flags_test() ->
    %% Construct: DF=1 (Don't Fragment), MF=0, FragOff=0
    <<Flags:3, FragOff:13>> = <<2:3, 0:13>>,
    2 = Flags,   %% bit 1 = DF
    0 = FragOff,

    %% Construct: MF=1, FragOff=185
    <<F2:3, FO2:13>> = <<1:3, 185:13>>,
    1 = F2,
    185 = FO2,
    ok.
