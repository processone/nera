-module(probe).
-export([main/0]).
main()->bench(ascii12,fun()->setup(300000,12,65) end),
bench(ascii128,fun()->setup(100000,128,65) end),
bench(unicode32,fun()->setup(300000,32,233) end),
bench(unicode128,fun()->setup(100000,128,1114111) end),ok.
bench(N,F)->F(),T=erlang:monotonic_time(microsecond),F(),D=erlang:monotonic_time(microsecond)-T,io:format("~p ~p~n",[N,D]).
setup(N,L,C)->S=lists:duplicate(L,C),E=list_to_atom(S),loop(N,S,E).
loop(0,_,_)->ok;
loop(N,S,E)->true=(list_to_atom(S)=:=E),loop(N-1,S,E).
