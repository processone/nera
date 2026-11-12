-module(probe).
-export([main/0]).
main() ->
    bench(f16_write, fun() -> write_f16(500000,1.5,3,64,<<1:3,<<62,0>>/binary,3:2>>) end),
    bench(f16_read, fun() -> read_f16(500000,1.5,3,64,<<1:3,<<62,0>>/binary,3:2>>) end),
    bench(f16le_write, fun() -> write_f16le(500000,1.5,3,64,<<1:3,<<0,62>>/binary,3:2>>) end),
    bench(f16le_read, fun() -> read_f16le(500000,1.5,3,64,<<1:3,<<0,62>>/binary,3:2>>) end),
    ok.
bench(Name,F) -> F(), T=erlang:monotonic_time(microsecond), F(), D=erlang:monotonic_time(microsecond)-T, io:format("~p ~p~n",[Name,D]).
write_f16(0,_,_,_,_) -> ok;
write_f16(I,X,P,N,Expected) -> Expected = <<1:P,X:16/float,3:2>>, write_f16(I-1,X,P,N,Expected).
read_f16(0,_,_,_,_) -> ok;
read_f16(I,X,P,N,Bin) -> <<_:P,X:16/float,3:2>> = Bin, read_f16(I-1,X,P,N,Bin).
write_f16le(0,_,_,_,_) -> ok;
write_f16le(I,X,P,N,Expected) -> Expected = <<1:P,X:16/float-little,3:2>>, write_f16le(I-1,X,P,N,Expected).
read_f16le(0,_,_,_,_) -> ok;
read_f16le(I,X,P,N,Bin) -> <<_:P,X:16/float-little,3:2>> = Bin, read_f16le(I-1,X,P,N,Bin).
