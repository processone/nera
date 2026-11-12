import subprocess, statistics, json, os, hashlib, time
from pathlib import Path
root=Path(__file__).resolve().parent
paths={k:root/k for k in ['before-aligned','after-aligned','after-shifted']}
def digest(p): return hashlib.sha256(p.read_bytes()).hexdigest()
result={'operations_per_case':100000,'warmups_per_case':100000,'series':[],'start_load':os.getloadavg(),'executables':{k:digest(p) for k,p in paths.items()},'timestamp':time.time()}
for series in range(3):
    values={k:[] for k in paths}
    for pair in range(9):
        order=list(paths)
        if pair%2: order.reverse()
        order=order[series:]+order[:series]
        for key in order:
            p=subprocess.run([str(paths[key]),'+S','1'],capture_output=True,text=True,check=True)
            sample={}
            for line in p.stdout.splitlines():
                name,value=line.split(); sample[name]=int(value)
            assert len(sample)==12, p.stdout
            values[key].append(sample)
    result['series'].append(values)
    (root/'performance.json').write_text(json.dumps(result,indent=2)+'\n')
    print('Completed series',series+1,flush=True)
result['end_load']=os.getloadavg()
summary={}
for name in result['series'][0]['before-aligned'][0]:
    med={key:statistics.median(s[name] for series in result['series'] for s in series[key]) for key in paths}
    changes=[]
    for series in result['series']:
        a=statistics.median(s[name] for s in series['before-aligned'])
        b=statistics.median(s[name] for s in series['after-aligned'])
        changes.append((b/a-1)*100)
    summary[name]={'median_us':med,'aligned_change_pct':(med['after-aligned']/med['before-aligned']-1)*100,'shifted_vs_aligned_pct':(med['after-shifted']/med['after-aligned']-1)*100,'aligned_series_pct':changes}
result['summary']=summary
(root/'performance.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(summary,indent=2),flush=True)
