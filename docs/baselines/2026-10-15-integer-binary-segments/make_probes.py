from pathlib import Path
root=Path('/tmp/nera-integer-segment-evidence')
cases=[('i8',8,42,''),('i32',32,305419896,''),('i64',64,305419896,''),('i64le',64,305419896,'/little'),('i13',13,2748,''),('dynamic32',32,305419896,''),('dynamicprefix32',32,305419896,''),('binary16',128,None,'/binary'),('binary256',2048,None,'/binary')]
extra=[('i13le',13,2748,'/little'),('i80',80,1208925819614629174706175,''),('i128le',128,170141183460469231731687303715884105731,'/little')]
for group,cs in [('common',cases),('extra',extra)]:
 for offset in [0,3]:
  d=root/f'{group}-{offset}';d.mkdir(exist_ok=True)
  mains=[]; funcs=[]
  for name,bits,x,flags in cs:
   isbin=x is None
   if isbin: src=list(range(bits//8));xexpr='<<'+','.join(map(str,src))+'>>'; wire=''.join(f'{v:08b}' for v in src)
   else:
    xexpr=str(x)
    if flags=='/little': wire=''.join(f'{(x>>i)&((1<<min(8,bits-i))-1):0{min(8,bits-i)}b}' for i in range(0,bits,8))
    else:wire=f'{x:0{bits}b}'
   prefix=f'{5&((1<<offset)-1):0{offset}b}' if offset else ''
   allbits=prefix+wire+'11'; n=len(allbits); raw=int(allbits,2)
   expected='<<'+','.join(str(int(allbits[i:i+8],2)) if len(allbits[i:i+8])==8 else str(int(allbits[i:i+8],2))+':'+str(len(allbits[i:i+8])) for i in range(0,n,8))+'>>'
   prefix_size='P' if name=='dynamicprefix32' else str(offset)
   size='N' if name=='dynamic32' else str(bits//8 if isbin else bits)
   if name=='binary256': segment='X/binary';readseg='V:256/binary'
   else: segment=f'X:{size}{flags}';readseg=f'V:{size}{flags}'
   for operation in ['write','read']:
    f=f'{name}_{operation}'
    iterations=50000 if name=='binary256' else 100000 if group=='extra' else 500000
    mains.append(f'    bench({f}, fun() -> {f}({iterations},{xexpr},{offset},{bits},{expected}) end)')
    if operation=='write': body=f'true=(Expected =:= <<5:{prefix_size},{segment},3:2>>)'
    else: body=f'<<_:{prefix_size},{readseg},3:2>>=Expected, true=(V=:=X)'
    funcs.extend([f'{f}(0,_,_,_,_) -> ok;',f'{f}(I,X,P,N,Expected) -> {body}, {f}(I-1,X,P,N,Expected).'])
  (d/'probe.erl').write_text('-module(probe).\n-export([main/0]).\nmain() ->\n'+',\n'.join(mains)+',\n    ok.\nbench(Name,F) -> F(), T=erlang:monotonic_time(microsecond), F(), D=erlang:monotonic_time(microsecond)-T, io:format("~p ~p~n",[Name,D]).\n'+'\n'.join(funcs)+'\n')
