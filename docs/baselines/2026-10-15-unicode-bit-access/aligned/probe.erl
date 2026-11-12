-module(probe).
-export([main/0]).
main() ->
    bench(u8_ascii_write, fun() -> write_u8_ascii(100000, 65, 0, <<1:0, <<65>>/binary, 3:2>>) end),
    bench(u8_ascii_read, fun() -> read_u8_ascii(100000, 65, 0, <<1:0, <<65>>/binary, 3:2>>) end),
    bench(u8_multi_write, fun() -> write_u8_multi(100000, 128512, 0, <<1:0, <<240,159,152,128>>/binary, 3:2>>) end),
    bench(u8_multi_read, fun() -> read_u8_multi(100000, 128512, 0, <<1:0, <<240,159,152,128>>/binary, 3:2>>) end),
    bench(u16_bmp_write, fun() -> write_u16_bmp(100000, 8364, 0, <<1:0, <<32,172>>/binary, 3:2>>) end),
    bench(u16_bmp_read, fun() -> read_u16_bmp(100000, 8364, 0, <<1:0, <<32,172>>/binary, 3:2>>) end),
    bench(u16_pair_write, fun() -> write_u16_pair(100000, 128512, 0, <<1:0, <<216,61,222,0>>/binary, 3:2>>) end),
    bench(u16_pair_read, fun() -> read_u16_pair(100000, 128512, 0, <<1:0, <<216,61,222,0>>/binary, 3:2>>) end),
    bench(u16_le_write, fun() -> write_u16_le(100000, 128512, 0, <<1:0, <<61,216,0,222>>/binary, 3:2>>) end),
    bench(u16_le_read, fun() -> read_u16_le(100000, 128512, 0, <<1:0, <<61,216,0,222>>/binary, 3:2>>) end),
    bench(u32_write, fun() -> write_u32(100000, 128512, 0, <<1:0, <<0,1,246,0>>/binary, 3:2>>) end),
    bench(u32_read, fun() -> read_u32(100000, 128512, 0, <<1:0, <<0,1,246,0>>/binary, 3:2>>) end),
    ok.
bench(Name, F) -> F(), T=erlang:monotonic_time(microsecond), F(), D=erlang:monotonic_time(microsecond)-T, io:format("~p ~p~n", [Name,D]).
write_u8_ascii(0, _, _, _) -> ok;
write_u8_ascii(N,C,P,Expected) -> Expected = <<1:P,C/utf8,3:2>>, write_u8_ascii(N-1,C,P,Expected).
read_u8_ascii(0, _, _, _) -> ok;
read_u8_ascii(N,C,P,Bin) -> <<_:P,C/utf8,3:2>> = Bin, read_u8_ascii(N-1,C,P,Bin).
write_u8_multi(0, _, _, _) -> ok;
write_u8_multi(N,C,P,Expected) -> Expected = <<1:P,C/utf8,3:2>>, write_u8_multi(N-1,C,P,Expected).
read_u8_multi(0, _, _, _) -> ok;
read_u8_multi(N,C,P,Bin) -> <<_:P,C/utf8,3:2>> = Bin, read_u8_multi(N-1,C,P,Bin).
write_u16_bmp(0, _, _, _) -> ok;
write_u16_bmp(N,C,P,Expected) -> Expected = <<1:P,C/utf16,3:2>>, write_u16_bmp(N-1,C,P,Expected).
read_u16_bmp(0, _, _, _) -> ok;
read_u16_bmp(N,C,P,Bin) -> <<_:P,C/utf16,3:2>> = Bin, read_u16_bmp(N-1,C,P,Bin).
write_u16_pair(0, _, _, _) -> ok;
write_u16_pair(N,C,P,Expected) -> Expected = <<1:P,C/utf16,3:2>>, write_u16_pair(N-1,C,P,Expected).
read_u16_pair(0, _, _, _) -> ok;
read_u16_pair(N,C,P,Bin) -> <<_:P,C/utf16,3:2>> = Bin, read_u16_pair(N-1,C,P,Bin).
write_u16_le(0, _, _, _) -> ok;
write_u16_le(N,C,P,Expected) -> Expected = <<1:P,C/utf16-little,3:2>>, write_u16_le(N-1,C,P,Expected).
read_u16_le(0, _, _, _) -> ok;
read_u16_le(N,C,P,Bin) -> <<_:P,C/utf16-little,3:2>> = Bin, read_u16_le(N-1,C,P,Bin).
write_u32(0, _, _, _) -> ok;
write_u32(N,C,P,Expected) -> Expected = <<1:P,C/utf32,3:2>>, write_u32(N-1,C,P,Expected).
read_u32(0, _, _, _) -> ok;
read_u32(N,C,P,Bin) -> <<_:P,C/utf32,3:2>> = Bin, read_u32(N-1,C,P,Bin).
