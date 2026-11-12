from pathlib import Path
import subprocess,os,json,hashlib,statistics
root=Path(__file__).resolve().parent
names=['before','after']
operations=dict(binary0=1000000,binary1=1000000,binary16=300000,binary256=20000,binary4096=2000,tuple0=2000000,tuple16=300000,tuple256=30000,list16=300000,map16=100000)
result={'series':3,'groups_per_series':5,'scheduler_threads':1,'warmup':'same work as timed sample','operations':operations,'load_start':os.getloadavg(),'hashes':{n:hashlib.sha256((root/n).read_bytes()).hexdigest() for n in names},'observations':[]}
for s in range(3):
 for g in range(5):
  order=names if (s+g)%2==0 else names[::-1]
  for n in order:
   output=subprocess.check_output([str(root/n),'+S','1'],text=True,stderr=subprocess.STDOUT,timeout=90)
   values={k:int(v) for k,v in (line.split() for line in output.splitlines())}
   assert set(values)==set(operations),output
   result['observations'].append({'series':s,'group':g,'executable':n,'values_us':values,'load':os.getloadavg()})
  print('series',s+1,'group',g+1,'complete',flush=True)
result['load_end']=os.getloadavg()
result['medians_us']={n:{k:statistics.median(o['values_us'][k] for o in result['observations'] if o['executable']==n) for k in operations} for n in names}
result['series_medians_us']={str(s):{n:{k:statistics.median(o['values_us'][k] for o in result['observations'] if o['executable']==n and o['series']==s) for k in operations} for n in names} for s in range(3)}
(root/'performance.json').write_text(json.dumps(result,indent=2)+'\n')
for k,b in result['medians_us']['before'].items():
 a=result['medians_us']['after'][k];print(k,b,a,round((a/b-1)*100,2),'%',flush=True)
print('load',result['load_start'],result['load_end'],flush=True)
