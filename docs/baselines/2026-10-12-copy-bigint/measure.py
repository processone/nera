"""Usage: python3 measure.py BEFORE_BINARY AFTER_BINARY > results.json"""
import json, resource, statistics, subprocess, sys
paths = dict(zip(('before', 'after'), sys.argv[1:]))
assert len(paths) == 2
samples = {name: [] for name in paths}
def run(name):
    start = resource.getrusage(resource.RUSAGE_CHILDREN)
    lines = subprocess.check_output([paths[name]], text=True, timeout=30).splitlines()
    end = resource.getrusage(resource.RUSAGE_CHILDREN)
    row = {kind: int(us) for kind, us in (line.split() for line in lines)}
    assert len(row) == 2, lines
    row['process_cpu_us'] = round(1e6 * (end.ru_utime + end.ru_stime - start.ru_utime - start.ru_stime))
    return row
for name in paths:
    run(name)
for pair in range(9):
    for name in (['before', 'after'] if pair % 2 == 0 else ['after', 'before']):
        samples[name].append(run(name))
medians = {name: {metric: statistics.median(row[metric] for row in rows)
    for metric in rows[0]} for name, rows in samples.items()}
print(json.dumps({'samples': samples, 'medians': medians,
    'percent_change': {metric: 100*(medians['after'][metric]/medians['before'][metric]-1)
    for metric in medians['before']}}, indent=2))
