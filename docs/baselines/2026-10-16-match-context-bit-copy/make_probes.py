from pathlib import Path
root=Path(__file__).resolve().parent

def literal(bits):
 return '<<'+','.join(str(int(bits[i:i+8],2))+(':'+str(len(bits[i:i+8])) if len(bits[i:i+8])<8 else '') for i in range(0,len(bits),8))+'>>'
for offset in (0,3):
 d=root/f'probe-{offset}';d.mkdir(exist_ok=True)
 mains=[];funcs=[]
 for name,bits,iterations in [('copy1',8,500000),('copy13bits',13,500000),('copy16',128,500000),('copy256',2048,50000)]:
  data=bytes(i%256 for i in range((bits+7)//8));wire=''.join(f'{b:08b}' for b in data)
  prefix=f'{5&((1<<offset)-1):0{offset}b}' if offset else ''
  x=literal(wire);expected=literal(prefix+wire[:bits]+'11')
  mains.append(f'bench({name},fun()->{name}({iterations},{x},{bits},{expected}) end)')
  funcs += [f'{name}(0,_,_,_)->ok;',f'{name}(I,X,N,E)->true=(E=:=<<5:{offset},X:N/bitstring,3:2>>),{name}(I-1,X,N,E).']
 # Dynamic matching window -> exclusive tail recursion, with checksum each time.
 wire=''.join(f'{i:08b}' for i in range(256));prefix=f'{5:03b}' if offset else ''
 mains.append(f'bench(scan256,fun()->repeat(10000,{literal(prefix+wire)},{offset}) end)')
 funcs += ['repeat(0,_,_)->ok;','repeat(I,B,P)->true=(start(B,P)=:=32640),repeat(I-1,B,P).',
 'start(B,P)-><<_:P,T/bitstring>>=B,scan(T,0).','scan(<<>>,A)->A;','scan(<<H:8,T/bitstring>>,A)->scan(T,A+H).']
 mains.append(f'bench(message256,fun()->message({literal(wire)}) end)')
 funcs += ['message(B)->{Pid,Ref}=spawn_monitor(fun worker/0),ping(10000,Pid,B),Pid!stop,receive {\'DOWN\',Ref,process,Pid,normal}->ok end.',
 'worker()->receive stop->ok;{From,B}->From!{self(),B},worker() end.',
 'ping(0,_,_)->ok;','ping(N,Pid,B)->Pid!{self(),B},receive {Who,Actual}->true=(Who=:=Pid),true=(Actual=:=B) end,ping(N-1,Pid,B).']
 (d/'probe.erl').write_text('-module(probe).\n-export([main/0]).\nmain()->'+',\n'.join(mains)+',ok.\nbench(N,F)->F(),T=erlang:monotonic_time(microsecond),F(),D=erlang:monotonic_time(microsecond)-T,io:format("~p ~p~n",[N,D]).\n'+'\n'.join(funcs)+'\n')
