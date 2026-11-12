from pathlib import Path
import subprocess,json,os,statistics,hashlib,resource
r=Path(__file__).resolve().parent
names=['before-scan-0','after-scan-0','before-scan-3','after-scan-3']
out={'operations':100000,'warmup_operations':100000,'series':3,'groups_per_series':5,'scheduler_threads':1,'load_start':os.getloadavg(),'hashes':{n:hashlib.sha256((r/n).read_bytes()).hexdigest() for n in names},'observations':[]}
for series in range(3):
 for group in range(5):
  order=names[:]
  if (series+group)%2:order.reverse()
  k=(series*5+group)%4;order=order[k:]+order[:k]
  for name in order:
   prior=resource.getrusage(resource.RUSAGE_CHILDREN)
   text=subprocess.check_output([str(r/name),'+S','1'],text=True,stderr=subprocess.STDOUT,timeout=60)
   after=resource.getrusage(resource.RUSAGE_CHILDREN)
   key,value=text.split();assert key=='scan256',text
   out['observations'].append({'series':series,'group':group,'executable':name,'timed_wall_us':int(value),'whole_process_user_seconds':after.ru_utime-prior.ru_utime,'whole_process_system_seconds':after.ru_stime-prior.ru_stime,'load':os.getloadavg()})
  print('series',series+1,'group',group+1,'complete',flush=True)
out['load_end']=os.getloadavg()
out['medians_us']={n:statistics.median(o['timed_wall_us'] for o in out['observations'] if o['executable']==n) for n in names}
out['median_user_seconds']={n:statistics.median(o['whole_process_user_seconds'] for o in out['observations'] if o['executable']==n) for n in names}
out['series_medians_us']={str(s):{n:statistics.median(o['timed_wall_us'] for o in out['observations'] if o['executable']==n and o['series']==s) for n in names} for s in range(3)}
(r/'performance-scan.json').write_text(json.dumps(out,indent=2)+'\n')
print(out['medians_us'],flush=True)
print('whole process user CPU (including warmup/startup)',out['median_user_seconds'],flush=True)
for p in [0,3]:print('offset',p,'change%',round((out['medians_us'][f'after-scan-{p}']/out['medians_us'][f'before-scan-{p}']-1)*100,2),flush=True)
print('load',out['load_start'],out['load_end'],flush=True)
