-module(probe).
-export([main/0]).
main() ->
    bench(f32_write, fun() -> write_f32(500000,1.5,0,64,<<1:0,<<63,192,0,0>>/binary,3:2>>) end),
    bench(f32_read, fun() -> read_f32(500000,1.5,0,64,<<1:0,<<63,192,0,0>>/binary,3:2>>) end),
    bench(f64_write, fun() -> write_f64(500000,1.5,0,64,<<1:0,<<63,248,0,0,0,0,0,0>>/binary,3:2>>) end),
    bench(f64_read, fun() -> read_f64(500000,1.5,0,64,<<1:0,<<63,248,0,0,0,0,0,0>>/binary,3:2>>) end),
    bench(f32le_write, fun() -> write_f32le(500000,1.5,0,64,<<1:0,<<0,0,192,63>>/binary,3:2>>) end),
    bench(f32le_read, fun() -> read_f32le(500000,1.5,0,64,<<1:0,<<0,0,192,63>>/binary,3:2>>) end),
    bench(f64le_write, fun() -> write_f64le(500000,1.5,0,64,<<1:0,<<0,0,0,0,0,0,248,63>>/binary,3:2>>) end),
    bench(f64le_read, fun() -> read_f64le(500000,1.5,0,64,<<1:0,<<0,0,0,0,0,0,248,63>>/binary,3:2>>) end),
    bench(f32int_write, fun() -> write_f32int(500000,42,0,64,<<1:0,<<66,40,0,0>>/binary,3:2>>) end),
    bench(f32int_read, fun() -> read_f32int(500000,42.0,0,64,<<1:0,<<66,40,0,0>>/binary,3:2>>) end),
    bench(f64big_write, fun() -> write_f64big(500000,1152921504606846976,0,64,<<1:0,<<67,176,0,0,0,0,0,0>>/binary,3:2>>) end),
    bench(f64big_read, fun() -> read_f64big(500000,1.152921504606847e+18,0,64,<<1:0,<<67,176,0,0,0,0,0,0>>/binary,3:2>>) end),
    bench(f64dynamic_write, fun() -> write_f64dynamic(500000,1.5,0,64,<<1:0,<<63,248,0,0,0,0,0,0>>/binary,3:2>>) end),
    bench(f64dynamic_read, fun() -> read_f64dynamic(500000,1.5,0,64,<<1:0,<<63,248,0,0,0,0,0,0>>/binary,3:2>>) end),
    ok.
bench(Name,F) -> F(), T=erlang:monotonic_time(microsecond), F(), D=erlang:monotonic_time(microsecond)-T, io:format("~p ~p~n",[Name,D]).
write_f32(0,_,_,_,_) -> ok;
write_f32(I,X,P,N,Expected) -> Expected = <<1:P,X:32/float,3:2>>, write_f32(I-1,X,P,N,Expected).
read_f32(0,_,_,_,_) -> ok;
read_f32(I,X,P,N,Bin) -> <<_:P,X:32/float,3:2>> = Bin, read_f32(I-1,X,P,N,Bin).
write_f64(0,_,_,_,_) -> ok;
write_f64(I,X,P,N,Expected) -> Expected = <<1:P,X:64/float,3:2>>, write_f64(I-1,X,P,N,Expected).
read_f64(0,_,_,_,_) -> ok;
read_f64(I,X,P,N,Bin) -> <<_:P,X:64/float,3:2>> = Bin, read_f64(I-1,X,P,N,Bin).
write_f32le(0,_,_,_,_) -> ok;
write_f32le(I,X,P,N,Expected) -> Expected = <<1:P,X:32/float-little,3:2>>, write_f32le(I-1,X,P,N,Expected).
read_f32le(0,_,_,_,_) -> ok;
read_f32le(I,X,P,N,Bin) -> <<_:P,X:32/float-little,3:2>> = Bin, read_f32le(I-1,X,P,N,Bin).
write_f64le(0,_,_,_,_) -> ok;
write_f64le(I,X,P,N,Expected) -> Expected = <<1:P,X:64/float-little,3:2>>, write_f64le(I-1,X,P,N,Expected).
read_f64le(0,_,_,_,_) -> ok;
read_f64le(I,X,P,N,Bin) -> <<_:P,X:64/float-little,3:2>> = Bin, read_f64le(I-1,X,P,N,Bin).
write_f32int(0,_,_,_,_) -> ok;
write_f32int(I,X,P,N,Expected) -> Expected = <<1:P,X:32/float,3:2>>, write_f32int(I-1,X,P,N,Expected).
read_f32int(0,_,_,_,_) -> ok;
read_f32int(I,X,P,N,Bin) -> <<_:P,X:32/float,3:2>> = Bin, read_f32int(I-1,X,P,N,Bin).
write_f64big(0,_,_,_,_) -> ok;
write_f64big(I,X,P,N,Expected) -> Expected = <<1:P,X:64/float,3:2>>, write_f64big(I-1,X,P,N,Expected).
read_f64big(0,_,_,_,_) -> ok;
read_f64big(I,X,P,N,Bin) -> <<_:P,X:64/float,3:2>> = Bin, read_f64big(I-1,X,P,N,Bin).
write_f64dynamic(0,_,_,_,_) -> ok;
write_f64dynamic(I,X,P,N,Expected) -> Expected = <<1:P,X:N/float,3:2>>, write_f64dynamic(I-1,X,P,N,Expected).
read_f64dynamic(0,_,_,_,_) -> ok;
read_f64dynamic(I,X,P,N,Bin) -> <<_:P,X:N/float,3:2>> = Bin, read_f64dynamic(I-1,X,P,N,Bin).
