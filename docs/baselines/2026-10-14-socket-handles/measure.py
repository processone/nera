"""Alternate nine pairs for exact-registry and native TCP probes."""
import datetime, hashlib, json, os, pathlib, re, statistics, subprocess, sys
output = pathlib.Path(sys.argv[1])
samples = {label: [{} for _ in range(9)] for label in ['before','after']}
start = {'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(), 'load':os.getloadavg()}
def run(label, native):
    binary = f'/tmp/nera-socket-{"native-" if native else ""}{label}'
    r = subprocess.run([binary],text=True,capture_output=True,check=True,
        env={**os.environ,'NERA_SCHEDULERS':'1'},timeout=60)
    parsed = {key:int(value) for key,value in re.findall(r'^(lookup_[1248]|reuse|fresh|registration|roundtrip) (\d+)$',r.stdout,re.M)}
    assert len(parsed)==(2 if native else 6),r.stdout
    if not native:
        assert re.search(r'^slot_bytes 16$',r.stdout,re.M), r.stdout
    return parsed
for native in [False,True]:
    for label in ['before','after']: run(label,native)
    for pair in range(9):
        order = ['before','after'] if pair%2==0 else ['after','before']
        for label in order: samples[label][pair].update(run(label,native))
medians = {label:{key:statistics.median(row[key] for row in rows) for key in rows[0]} for label,rows in samples.items()}
changes = {key:(medians['after'][key]/value-1)*100 for key,value in medians['before'].items()}
paired = {key:statistics.median((a[key]/b[key]-1)*100 for a,b in zip(samples['after'],samples['before'])) for key in changes}
files=[f'/tmp/nera-socket-{prefix}{label}' for prefix in ['', 'native-'] for label in ['before','after']]
output.write_text(json.dumps({'samples_us':samples,'medians_us':medians,'percent_change':changes,'paired_percent_change':paired,
    'start':start,'end':{'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'load':os.getloadavg()},
    'slot_bytes':16,'binary_hashes':{f:hashlib.sha256(pathlib.Path(f).read_bytes()).hexdigest() for f in files}},indent=2)+'\n')
print(json.dumps({'percent_change':changes,'paired_percent_change':paired},indent=2))
