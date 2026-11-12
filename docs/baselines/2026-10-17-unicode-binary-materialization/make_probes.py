from pathlib import Path
root=Path(__file__).resolve().parent
# Reuse the now-valid capture workload from #109, and add boundary/partial windows.
for p in [0,3]:
 d=root/f'materialize-{p}';d.mkdir(exist_ok=True)
 mains=[]
 for label,bits,n in [('partial13',13,300000),('bytes16',128,300000),('bytes64',512,100000),('bytes65',520,100000),('bytes256',2048,100000)]:
  raw=''.join(f'{(i*71+0xa5)%256:08b}' for i in range((bits+7)//8))[:bits]
  def lit(wire):return '<<'+','.join(str(int(wire[i:i+8],2))+(':'+str(len(wire[i:i+8])) if len(wire[i:i+8])<8 else '') for i in range(0,len(wire),8))+'>>'
  prefix=f'{5&((1<<p)-1):0{p}b}' if p else ''
  mains.append(f'bench({label},fun()->loop({n},{lit(prefix+raw)},{p},{lit(raw)}) end)')
 (d/'probe.erl').write_text('-module(probe).\n-export([main/0]).\nmain()->'+',\n'.join(mains)+',ok.\nbench(N,F)->F(),T=erlang:monotonic_time(microsecond),F(),D=erlang:monotonic_time(microsecond)-T,io:format("~p ~p~n",[N,D]).\nloop(0,_,_,_)->ok;\nloop(N,B,P,E)->F=capture(B,P),true=(F()=:=E),loop(N-1,B,P,E).\ncapture(B,P)-><<_:P,T/bitstring>>=B,fun()->T end.\n')
d=root/'unicode';d.mkdir(exist_ok=True)
mains=[]
for name,length,cp,n in [('ascii12',12,65,300000),('ascii128',128,65,100000),('unicode32',32,233,300000),('unicode128',128,0x10ffff,100000)]:
 mains.append(f'bench({name},fun()->setup({n},{length},{cp}) end)')
(d/'probe.erl').write_text('-module(probe).\n-export([main/0]).\nmain()->'+',\n'.join(mains)+',ok.\nbench(N,F)->F(),T=erlang:monotonic_time(microsecond),F(),D=erlang:monotonic_time(microsecond)-T,io:format("~p ~p~n",[N,D]).\nsetup(N,L,C)->S=lists:duplicate(L,C),E=list_to_atom(S),loop(N,S,E).\nloop(0,_,_)->ok;\nloop(N,S,E)->true=(list_to_atom(S)=:=E),loop(N-1,S,E).\n')
