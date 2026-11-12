"""Nine alternating paired runs after warmup; elapsed times are microseconds."""
import json, os, pathlib, re, statistics, subprocess, sys
before, after, spawn_before, spawn_after, output = sys.argv[1:]

def run(probe, spawn):
    result = subprocess.run([probe, '--ignored', '--exact',
        'process::entry_performance_probe::process_entry_performance_probe', '--nocapture',
        '--test-threads=1'], check=True, text=True, capture_output=True, timeout=60)
    values = {key: int(value) for key, value in re.findall(r'\b(pid_[1248]) (\d+)$', result.stdout, re.M)}
    assert len(values) == 4, result.stdout
    for threads in [1, 2, 4, 8]:
        result = subprocess.run([spawn], env={**os.environ, 'NERA_SCHEDULERS': str(threads)},
            check=True, text=True, capture_output=True, timeout=60)
        match = re.search(r'^spawn_real (\d+)$', result.stdout, re.M)
        assert match, result.stdout
        values[f'spawn_{threads}'] = int(match[1])
    return values

run(before, spawn_before)
run(after, spawn_after)
samples = {'before': [], 'after': []}
for pair in range(9):
    order = [('before', before, spawn_before), ('after', after, spawn_after)]
    if pair % 2: order.reverse()
    for label, probe, spawn in order: samples[label].append(run(probe, spawn))
medians = {label: {key: statistics.median(row[key] for row in rows) for key in rows[0]}
    for label, rows in samples.items()}
changes = {key: (medians['after'][key] / value - 1) * 100 for key, value in medians['before'].items()}
pathlib.Path(output).write_text(json.dumps({'samples': samples, 'medians': medians,
    'percent_change': changes}, indent=2) + '\n')
print(json.dumps({'medians': medians, 'percent_change': changes}, indent=2))
