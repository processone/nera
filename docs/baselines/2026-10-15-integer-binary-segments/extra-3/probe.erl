-module(probe).
-export([main/0]).
main() ->
    bench(i13le_write, fun() -> i13le_write(100000,2748,3,13,<<183,138,3:2>>) end),
    bench(i13le_read, fun() -> i13le_read(100000,2748,3,13,<<183,138,3:2>>) end),
    bench(i80_write, fun() -> i80_write(100000,1208925819614629174706175,3,80,<<191,255,255,255,255,255,255,255,255,255,31:5>>) end),
    bench(i80_read, fun() -> i80_read(100000,1208925819614629174706175,3,80,<<191,255,255,255,255,255,255,255,255,255,31:5>>) end),
    bench(i128le_write, fun() -> i128le_write(100000,170141183460469231731687303715884105731,3,128,<<160,96,0,0,0,0,0,0,0,0,0,0,0,0,0,16,3:5>>) end),
    bench(i128le_read, fun() -> i128le_read(100000,170141183460469231731687303715884105731,3,128,<<160,96,0,0,0,0,0,0,0,0,0,0,0,0,0,16,3:5>>) end),
    ok.
bench(Name,F) -> F(), T=erlang:monotonic_time(microsecond), F(), D=erlang:monotonic_time(microsecond)-T, io:format("~p ~p~n",[Name,D]).
i13le_write(0,_,_,_,_) -> ok;
i13le_write(I,X,P,N,Expected) -> true=(Expected =:= <<5:3,X:13/little,3:2>>), i13le_write(I-1,X,P,N,Expected).
i13le_read(0,_,_,_,_) -> ok;
i13le_read(I,X,P,N,Expected) -> <<_:3,V:13/little,3:2>>=Expected, true=(V=:=X), i13le_read(I-1,X,P,N,Expected).
i80_write(0,_,_,_,_) -> ok;
i80_write(I,X,P,N,Expected) -> true=(Expected =:= <<5:3,X:80,3:2>>), i80_write(I-1,X,P,N,Expected).
i80_read(0,_,_,_,_) -> ok;
i80_read(I,X,P,N,Expected) -> <<_:3,V:80,3:2>>=Expected, true=(V=:=X), i80_read(I-1,X,P,N,Expected).
i128le_write(0,_,_,_,_) -> ok;
i128le_write(I,X,P,N,Expected) -> true=(Expected =:= <<5:3,X:128/little,3:2>>), i128le_write(I-1,X,P,N,Expected).
i128le_read(0,_,_,_,_) -> ok;
i128le_read(I,X,P,N,Expected) -> <<_:3,V:128/little,3:2>>=Expected, true=(V=:=X), i128le_read(I-1,X,P,N,Expected).
