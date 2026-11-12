-module(probe).
-export([main/0]).
main()->bench(binary0,fun()->E=lists:duplicate(0,165),B=list_to_binary(E),binary_loop(1000000,B,E) end),
bench(binary1,fun()->E=lists:duplicate(1,165),B=list_to_binary(E),binary_loop(1000000,B,E) end),
bench(binary16,fun()->E=lists:duplicate(16,165),B=list_to_binary(E),binary_loop(300000,B,E) end),
bench(binary256,fun()->E=lists:duplicate(256,165),B=list_to_binary(E),binary_loop(20000,B,E) end),
bench(binary4096,fun()->E=lists:duplicate(4096,165),B=list_to_binary(E),binary_loop(2000,B,E) end),
bench(tuple0,fun()->E={},B=term_to_binary(E),etf_loop(2000000,B,E) end),
bench(tuple16,fun()->E=list_to_tuple(lists:duplicate(16,[])),B=term_to_binary(E),etf_loop(300000,B,E) end),
bench(tuple256,fun()->E=list_to_tuple(lists:duplicate(256,[])),B=term_to_binary(E),etf_loop(30000,B,E) end),
bench(list16,fun()->E=lists:duplicate(16,[]),B=term_to_binary(E),etf_loop(300000,B,E) end),
bench(map16,fun()->E=maps:from_list([{X,[]} || X <- lists:seq(1,16)]),B=term_to_binary(E),etf_loop(100000,B,E) end),
bench(control_scalar,fun()->E=42,B=term_to_binary(E),etf_loop(2000000,B,E) end),
bench(control_bitstring16,fun()->E=lists:duplicate(16,165),B=list_to_binary(E),bitstring_loop(300000,B,E) end),ok.
bench(N,F)->F(),T=erlang:monotonic_time(microsecond),F(),D=erlang:monotonic_time(microsecond)-T,io:format("~p ~p~n",[N,D]).
binary_loop(0,_,_)->ok;
binary_loop(N,B,E)->true=(binary_to_list(B)=:=E),binary_loop(N-1,B,E).
etf_loop(0,_,_)->ok;
etf_loop(N,B,E)->true=(binary_to_term(B)=:=E),etf_loop(N-1,B,E).
bitstring_loop(0,_,_)->ok;
bitstring_loop(N,B,E)->true=(bitstring_to_list(B)=:=E),bitstring_loop(N-1,B,E).
