"""Alternate native binaries; measure six operations under 1/2/4/8 workers."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess

folder = Path(__file__).resolve().parent
cases = [('modes', '/tmp/nera-modes', 1, {'raw_once', 'line_once', 'line_active'}),
         ('recv', '/tmp/nera-modes-recv', 1, {'recv_1', 'recv_64', 'recv_4096', 'recv_1048576'})]
for threads in (1, 2, 4, 8):
    cases.append((f'arrays_{threads}', f'/tmp/nera-arrays-{threads}', threads,
                  {'read', 'add', 'add_get', 'cas', 'counter_read', 'counter_add'}))

def run(case, version):
    name, prefix, schedulers, keys = case
    result = subprocess.run([f'{prefix}-{version}'], check=True, capture_output=True,
        text=True, timeout=60, env={**os.environ, 'NERA_SCHEDULERS': str(schedulers)})
    samples = {key: int(value) for key, value in re.findall(r'^(\w+) (\d+)$', result.stdout, re.M)}
    assert set(samples) == keys, (name, result.stdout, result.stderr)
    return samples

output = {'start': {'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                    'load': os.getloadavg()}, 'cases': {}, 'binary_sha256': {}}
for case in cases:
    name, prefix, schedulers, keys = case
    series = []
    for batch in range(3):
        samples = {version: [] for version in ('before', 'after')}
        for version in samples:
            run(case, version)
        for pair in range(9):
            order = ('before', 'after') if (batch + pair) % 2 == 0 else ('after', 'before')
            for version in order:
                samples[version].append(run(case, version))
        series.append(samples)
    medians = {version: {key: statistics.median(row[key] for samples in series
        for row in samples[version]) for key in sorted(keys)} for version in ('before', 'after')}
    delta = {key: (medians['after'][key] / medians['before'][key] - 1) * 100 for key in sorted(keys)}
    series_delta = [{key: (statistics.median(row[key] for row in samples['after']) /
        statistics.median(row[key] for row in samples['before']) - 1) * 100 for key in sorted(keys)}
        for samples in series]
    output['cases'][name] = {'samples_us': series, 'medians_us': medians,
                            'percent_change': delta, 'series_percent_change': series_delta}
    for version in ('before', 'after'):
        binary = Path(f'{prefix}-{version}')
        output['binary_sha256'][str(binary)] = hashlib.sha256(binary.read_bytes()).hexdigest()
    output['end'] = {'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'load': os.getloadavg()}
    (folder / 'performance.json').write_text(json.dumps(output, indent=2) + '\n')
    print(json.dumps({'case': name, 'medians_us': medians, 'percent_change': delta,
                      'series_percent_change': series_delta}), flush=True)
