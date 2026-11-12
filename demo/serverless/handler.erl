-module(handler).
-export([start/0]).

%% Local serverless demo: parses two sample IPv4 packets compiled into
%% the program, using Erlang's bit-level binary pattern matching, and
%% prints each result as an Erlang term. It reads no event and is not a
%% qualified AWS Lambda handler; see docs/SERVERLESS_ERLANG.md.

start() ->
    Packets = sample_packets(),
    Results = lists:map(fun parse_packet/1, Packets),
    lists:foreach(fun(R) -> io:format("~p~n", [R]) end, Results),
    ok.

%% -- Packet parsing --------------------------------------------------------

parse_packet(Bin) ->
    {IP, Rest} = parse_ipv4(Bin),
    Proto = maps:get(protocol, IP),
    Transport = parse_transport(Proto, Rest),
    #{ip => IP, transport => Transport}.

parse_ipv4(<<Version:4, IHL:4, _DSCP:6, _ECN:2, TotalLen:16,
             _Id:16, _Flags:3, _FragOff:13,
             TTL:8, Protocol:8, _Checksum:16,
             S1:8, S2:8, S3:8, S4:8,
             D1:8, D2:8, D3:8, D4:8,
             Rest/binary>>) ->
    Info = #{version => Version,
             ihl => IHL,
             total_length => TotalLen,
             ttl => TTL,
             protocol => Protocol,
             src => {S1, S2, S3, S4},
             dst => {D1, D2, D3, D4}},
    {Info, Rest}.

%% TCP (protocol 6)
parse_transport(6, <<SrcPort:16, DstPort:16,
                     Seq:32, Ack:32,
                     _DataOff:4, _Reserved:3, Flags:9,
                     Window:16, _Rest/binary>>) ->
    #{type => tcp,
      src_port => SrcPort,
      dst_port => DstPort,
      seq => Seq,
      ack => Ack,
      flags => decode_tcp_flags(Flags),
      window => Window};

%% UDP (protocol 17)
parse_transport(17, <<SrcPort:16, DstPort:16,
                      Length:16, _Checksum:16,
                      _Rest/binary>>) ->
    #{type => udp,
      src_port => SrcPort,
      dst_port => DstPort,
      length => Length};

parse_transport(Proto, _) ->
    #{type => unknown, protocol_number => Proto}.

decode_tcp_flags(Flags) ->
    F = fun(Mask) ->
        case Flags band Mask of
            0 -> false;
            _ -> true
        end
    end,
    #{syn => F(2), ack => F(16), fin => F(1),
      rst => F(4), psh => F(8), urg => F(32)}.

%% -- Sample packets ---------------------------------------------------------

sample_packets() ->
    [tcp_syn_packet(), udp_dns_packet()].

%% TCP SYN: 172.16.10.99:49153 -> 172.16.10.12:80
tcp_syn_packet() ->
    <<
      %% IPv4 header (20 bytes)
      69, 0, 0, 60,          %% ver=4, ihl=5, len=60
      28, 70, 64, 0,         %% id, flags=DF, frag=0
      64, 6, 177, 230,       %% ttl=64, proto=TCP(6), checksum
      172, 16, 10, 99,       %% src
      172, 16, 10, 12,       %% dst
      %% TCP header (20 bytes)
      192, 1, 0, 80,         %% src_port=49153, dst_port=80
      0, 0, 0, 1,            %% seq=1
      0, 0, 0, 0,            %% ack=0
      160, 2, 255, 255,      %% data_off=10, flags=SYN, window=65535
      0, 0, 0, 0             %% checksum, urgent
    >>.

%% UDP DNS query: 10.0.0.5:12345 -> 8.8.8.8:53
udp_dns_packet() ->
    <<
      %% IPv4 header (20 bytes)
      69, 0, 0, 36,          %% ver=4, ihl=5, len=36
      0, 1, 0, 0,            %% id=1, no flags
      128, 17, 0, 0,         %% ttl=128, proto=UDP(17), checksum
      10, 0, 0, 5,           %% src
      8, 8, 8, 8,            %% dst
      %% UDP header (8 bytes)
      48, 57, 0, 53,         %% src_port=12345, dst_port=53
      0, 16, 0, 0            %% length=16, checksum
    >>.
