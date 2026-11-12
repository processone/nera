from pathlib import Path
import subprocess,os,json,hashlib,statistics
root=Path(__file__).resolve().parent
names=['before-unicode','after-unicode','before-0','after-0','before-3','after-3']
result={'series':3,'groups_per_series':5,'scheduler_threads':1,'warmup':'same work as timed sample','operations':{'ascii12':300000,'ascii128':100000,'unicode32':300000,'unicode128':100000,'partial13':300000,'bytes16':300000,'bytes64':100000,'bytes65':100000,'bytes256':100000},'load_start':os.getloadavg(),'hashes':{n:hashlib.sha256((root/n).read_bytes()).hexdigest() for n in names},'observations':[]}
for s in range(3):
 for g in range(5):
  order=names[:]
  if (s+g)%2:order.reverse()
  k=(s*5+g)%len(order);order=order[k:]+order[:k]
  for n in order:
   output=subprocess.check_output([str(root/n),'+S','1'],text=True,stderr=subprocess.STDOUT,timeout=90)
   values={k:int(v) for k,v in (line.split() for line in output.splitlines())}
   expected={'ascii12','ascii128','unicode32','unicode128'} if 'unicode' in n else {'partial13','bytes16','bytes64','bytes65','bytes256'}
   assert set(values)==expected,output
   result['observations'].append({'series':s,'group':g,'executable':n,'values_us':values,'load':os.getloadavg()})
  print('series',s+1,'group',g+1,'complete',flush=True)
result['load_end']=os.getloadavg()
result['medians_us']={n:{k:statistics.median(o['values_us'][k] for o in result['observations'] if o['executable']==n) for k in next(o['values_us'] for o in result['observations'] if o['executable']==n)} for n in names}
result['series_medians_us']={str(s):{n:{k:statistics.median(o['values_us'][k] for o in result['observations'] if o['executable']==n and o['series']==s) for k in result['medians_us'][n]} for n in names} for s in range(3)}
(root/'performance.json').write_text(json.dumps(result,indent=2)+'\n')
for group in ['unicode','0','3']:
 print('group',group,flush=True)
 for k,b in result['medians_us']['before-'+group].items():
  a=result['medians_us']['after-'+group][k];print(k,b,a,round((a/b-1)*100,2),'%',flush=True)
print('load',result['load_start'],result['load_end'],flush=True)
