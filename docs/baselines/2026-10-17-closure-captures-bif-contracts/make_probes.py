from pathlib import Path
root=Path(__file__).resolve().parent
for group in ['common','capture']:
 for offset in [0,3]:
  d=root/f'{group}-{offset}';d.mkdir(exist_ok=True)
  mains=[]
  for length,it in [(16,300000),(256,100000)]:
   data='<<'+','.join(map(str,range(length)))+'>>'
   if group=='common':mains.append(f'bench(plain{length},fun()->plain_loop({it},{data}) end)')
   else:
    mains.append(f'bench(capture{length},fun()->capture_loop({it},<<5:{offset},{data}/bitstring>>,{offset},{data}) end)')
  if group=='common':
   mains += ['bench(length1,fun()->length_loop(1000000,[1],1) end)', 'bench(length32,fun()->length_loop(300000,lists:seq(1,32),32) end)', 'bench(tuple_ops,fun()->tuple_loop(1000000,{1,2,3}) end)', 'bench(string_atom,fun()->string_loop(300000,"capture_bench",capture_bench) end)',f'bench(scan256,fun()->scan_loop(10000,<<5:{offset},'+','.join(map(str,range(256)))+f'>>,{offset}) end)']
  funcs='''
plain(B)->fun()->B end.
plain_loop(0,_)->ok;
plain_loop(N,B)->F=plain(B),true=(F()=:=B),plain_loop(N-1,B).
length_loop(0,_,_)->ok;
length_loop(N,L,E)->true=(length(L)=:=E),length_loop(N-1,L,E).
tuple_loop(0,_)->ok;
tuple_loop(N,T)->true=(tuple_size(T)=:=3),true=(element(2,T)=:=2),tuple_loop(N-1,T).
string_loop(0,_,_)->ok;
string_loop(N,S,A)->true=(list_to_atom(S)=:=A),string_loop(N-1,S,A).
scan_loop(0,_,_)->ok;
scan_loop(N,B,P)->true=(start(B,P)=:=32640),scan_loop(N-1,B,P).
start(B,P)-><<_:P,T/bitstring>>=B,scan(T,0).
scan(<<>>,A)->A;
scan(<<H:8,T/bitstring>>,A)->scan(T,A+H).
capture_loop(0,_,_,_)->ok;
capture_loop(N,B,P,E)->F=capture(B,P),true=(F()=:=E),capture_loop(N-1,B,P,E).
capture(B,P)-><<_:P,T/bitstring>>=B,fun()->T end.
'''
  (d/'probe.erl').write_text('-module(probe).\n-export([main/0]).\nmain()->'+',\n'.join(mains)+',ok.\nbench(N,F)->F(),T=erlang:monotonic_time(microsecond),F(),D=erlang:monotonic_time(microsecond)-T,io:format("~p ~p~n",[N,D]).\n'+funcs if group=='capture' else '-module(probe).\n-export([main/0]).\nmain()->'+',\n'.join(mains)+',ok.\nbench(N,F)->F(),T=erlang:monotonic_time(microsecond),F(),D=erlang:monotonic_time(microsecond)-T,io:format("~p ~p~n",[N,D]).\n'+funcs[:funcs.index('capture_loop')])
