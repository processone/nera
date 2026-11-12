"""python3 measure.py BEFORE_TEST_BINARY AFTER_TEST_BINARY > results.json"""
import json, resource, statistics, subprocess, sys
paths = dict(zip(('before', 'after'), sys.argv[1:]))
assert len(paths) == 2
metrics = {'header_static', 'header_dynamic', 'header_resize', 'heap_boxed'}
samples = {name: [] for name in paths}
def run(name):
    start = resource.getrusage(resource.RUSAGE_CHILDREN)
    output = subprocess.check_output([paths[name], '--ignored', 'process_entry_performance_probe', '--nocapture'], text=True, timeout=30)
    end = resource.getrusage(resource.RUSAGE_CHILDREN)
    row = {parts[0]: int(parts[1]) for line in output.splitlines()
           if len(parts := line.split()) == 2 and parts[0] in metrics}
    assert set(row) == metrics, output
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
