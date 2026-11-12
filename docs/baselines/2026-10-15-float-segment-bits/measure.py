import subprocess,statistics,json,os,hashlib,time
from pathlib import Path
r=Path(__file__).resolve().parent
paths={k:r/k for k in ['before-aligned','after-aligned','after-shifted','after-half-aligned','after-half-shifted']}
def run(k):
 p=subprocess.run([str(paths[k]),'+S','1'],capture_output=True,text=True,check=True)
 sample=dict((n,int(t)) for n,t in (line.split() for line in p.stdout.splitlines()))
 assert len(sample)==(4 if 'half' in k else 14),p.stdout
 return sample
out={'operations':500000,'warmups':500000,'series':[],'start_load':os.getloadavg(),'time':time.time(),'executables':{k:hashlib.sha256(p.read_bytes()).hexdigest() for k,p in paths.items()}}
for series in range(3):
 data={k:[] for k in paths}
 for pair in range(9):
  order=list(paths)
  if pair%2: order.reverse()
  order=order[series:]+order[:series]
  for key in order: data[key].append(run(key))
 out['series'].append(data)
 (r/'performance.json').write_text(json.dumps(out,indent=2)+'\n')
 print('Completed series',series+1,flush=True)
summary={}
for name in out['series'][0]['before-aligned'][0]:
 med={k:statistics.median(v[name] for s in out['series'] for v in s[k]) for k in list(paths)[:3]}
 changes=[]
 for s in out['series']:
  a=statistics.median(v[name] for v in s['before-aligned']);b=statistics.median(v[name] for v in s['after-aligned'])
  changes.append((b/a-1)*100)
 summary[name]={'median_us':med,'aligned_pct':(med['after-aligned']/med['before-aligned']-1)*100,'shifted_vs_aligned_pct':(med['after-shifted']/med['after-aligned']-1)*100,'aligned_series_pct':changes}
half={}
for name in out['series'][0]['after-half-aligned'][0]:
 med={k:statistics.median(v[name] for s in out['series'] for v in s[k]) for k in list(paths)[3:]}
 half[name]={'median_us':med,'shifted_vs_aligned_pct':(med['after-half-shifted']/med['after-half-aligned']-1)*100}
out.update(summary=summary,half=half,end_load=os.getloadavg())
(r/'performance.json').write_text(json.dumps(out,indent=2)+'\n')
print(json.dumps({'summary':summary,'half':half},indent=2),flush=True)
