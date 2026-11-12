import subprocess,os,hashlib,json,statistics
from pathlib import Path
root=Path(__file__).resolve().parent
names=['before-0','after-0','before-3','after-3','after-capture-0','after-capture-3']
data={n:{} for n in names}
result={'operations_by_case':{'plain16':300000,'plain256':100000,'length1':1000000,'length32':300000,'tuple_ops':1000000,'string_atom':300000,'scan256':10000,'capture16':300000,'capture256':100000},'warmup_operations':'same as timed operations','series':3,'groups_per_series':5,'scheduler_threads':1,'offsets':[0,3],'hashes':{n:hashlib.sha256((root/n).read_bytes()).hexdigest() for n in names},'load_start':os.getloadavg(),'observations':[]}
for series in range(3):
 for group in range(5):
  order=names[:]
  if (series+group)%2:order.reverse()
  rotate=(series*5+group)%len(order);order=order[rotate:]+order[:rotate]
  for name in order:
   output=subprocess.check_output([str(root/name),'+S','1'],text=True,stderr=subprocess.STDOUT,timeout=60)
   values={}
   for line in output.splitlines():
    k,v=line.split();values[k]=int(v);data[name].setdefault(k,[]).append(int(v))
   expected={'capture16','capture256'} if 'capture' in name else {'plain16','plain256','length1','length32','tuple_ops','string_atom','scan256'}
   assert set(values)==expected,output
   result['observations'].append({'series':series,'group':group,'executable':name,'load':os.getloadavg(),'values_us':values})
  print('series',series+1,'group',group+1,'complete',flush=True)
  (root/'performance-progress.json').write_text(json.dumps(result,indent=2)+'\n')
result['load_end']=os.getloadavg();result['medians_us']={n:{k:statistics.median(v) for k,v in cases.items()} for n,cases in data.items()}
result['series_medians_us']={str(s):{n:{k:statistics.median(o['values_us'][k] for o in result['observations'] if o['series']==s and o['executable']==n) for k in data[n]} for n in names} for s in range(3)}
(root/'performance.json').write_text(json.dumps(result,indent=2)+'\n')
for k in result['medians_us']['before-0']:
 vals=[result['medians_us'][n][k] for n in names[:4]];b,a,bs,ass=vals
 print(k,*vals,'cohort0%',round((a/b-1)*100,2),'cohort3%',round((ass/bs-1)*100,2),flush=True)
for n in names[4:]:print(n,result['medians_us'][n],flush=True)
print('load',result['load_start'],result['load_end'],flush=True)
