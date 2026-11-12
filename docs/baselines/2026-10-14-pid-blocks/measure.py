"""Nine alternating paired runs after warmup; elapsed times are microseconds.
Counter and real-spawn series are separate to avoid carrying the synthetic
counter's very different CPU load into the real-spawn measurements.
"""
import json, os, pathlib, re, statistics, subprocess, sys
before, after, spawn_before, spawn_after, output = sys.argv[1:]

def counter(probe):
    result = subprocess.run([probe, '--ignored', '--exact',
        'process::entry_performance_probe::process_entry_performance_probe', '--nocapture',
        '--test-threads=1'], check=True, text=True, capture_output=True, timeout=60)
    values = {key: int(value) for key, value in re.findall(r'\b(pid_[1248]) (\d+)$', result.stdout, re.M)}
    assert len(values) == 4, result.stdout
    return values

def spawn(binary, threads):
    result = subprocess.run([binary], env={**os.environ, 'NERA_SCHEDULERS': str(threads)},
        check=True, text=True, capture_output=True, timeout=60)
    match = re.search(r'^spawn_real (\d+)$', result.stdout, re.M)
    assert match, result.stdout
    return {f'spawn_{threads}': int(match[1])}

samples = {label: [{} for _ in range(9)] for label in ['before', 'after']}
operations = [(counter, before, after, ())]
operations += [(spawn, spawn_before, spawn_after, (threads,)) for threads in [1, 2, 4, 8]]
for function, old, new, args in operations:
    function(old, *args)
    function(new, *args)
    for pair in range(9):
        order = [('before', old), ('after', new)]
        if pair % 2: order.reverse()
        for label, binary in order: samples[label][pair].update(function(binary, *args))
medians = {label: {key: statistics.median(row[key] for row in rows) for key in rows[0]}
    for label, rows in samples.items()}
changes = {key: (medians['after'][key] / value - 1) * 100 for key, value in medians['before'].items()}
pathlib.Path(output).write_text(json.dumps({'samples': samples, 'medians': medians,
    'percent_change': changes}, indent=2) + '\n')
print(json.dumps({'medians': medians, 'percent_change': changes}, indent=2))
