"""Native Erlang FFI probes: three independent series, nine alternating pairs."""
import datetime, hashlib, json, os, pathlib, re, statistics, subprocess, sys
output = pathlib.Path(sys.argv[1])
repetitions = int(sys.argv[2]) if len(sys.argv) > 2 else 1
workers = [int(n) for n in sys.argv[3].split(',')] if len(sys.argv) > 3 else [1, 2, 4, 8]
assert repetitions > 0 and all(n in [1, 2, 4, 8] for n in workers)
samples = {label: [{} for _ in range(9)] for label in ['before', 'after']}
start = {'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'load': os.getloadavg()}
def run(label, threads):
    binary = f'/tmp/nera-atomics-{label}-{threads}'
    result = subprocess.run([binary], env={**os.environ, 'NERA_SCHEDULERS': str(threads)},
        text=True, capture_output=True, check=True, timeout=60)
    values = {f'{name}_{threads}': int(value) for name, value in
        re.findall(r'^(read|add|add_get|cas) (\d+)$', result.stdout, re.M)}
    assert len(values) == 4, result.stdout
    return values
for threads in workers:
    for _ in range(3 if repetitions > 1 else 1):
        run('before', threads); run('after', threads)
    for pair in range(9):
        for repetition in range(repetitions):
            order = ['before', 'after'] if (pair + repetition) % 2 == 0 else ['after', 'before']
            for label in order:
                for key, value in run(label, threads).items():
                    samples[label][pair][key] = samples[label][pair].get(key, 0) + value
medians = {label: {key: statistics.median(row[key] for row in rows) for key in rows[0]}
    for label, rows in samples.items()}
changes = {key: (medians['after'][key] / value - 1) * 100 for key, value in medians['before'].items()}
binaries = {f'{label}_{threads}': hashlib.sha256(pathlib.Path(f'/tmp/nera-atomics-{label}-{threads}').read_bytes()).hexdigest()
    for label in ['before', 'after'] for threads in [1, 2, 4, 8]}
output.write_text(json.dumps({'samples_us': samples, 'repetitions_per_sample': repetitions, 'workers': workers, 'medians_us': medians,
    'percent_change': changes, 'start': start, 'end': {
    'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'load': os.getloadavg()},
    'binary_hashes': binaries}, indent=2) + '\n')
print(json.dumps({'medians_us': medians, 'percent_change': changes}, indent=2))
