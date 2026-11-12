"""Alternate before/after native receive and TCP mode probes on the same host."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess

folder = Path(__file__).resolve().parent
cases = [('receive', '/tmp/nera-receive', {'plain', 'after_zero', 'after_finite', 'after_infinity', 'empty_poll'}),
         ('modes', '/tmp/nera-timeout-modes', {'raw_once', 'line_once', 'line_active'})]

def run(prefix, version, keys):
    result = subprocess.run([f'{prefix}-{version}'], check=True, capture_output=True,
        text=True, timeout=30, env={**os.environ, 'NERA_SCHEDULERS': '1'})
    rows = {key: int(value) for key, value in re.findall(r'^(\w+) (\d+)$', result.stdout, re.M)}
    assert set(rows) == keys, (result.stdout, result.stderr)
    return rows

output = {'start': {'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'load': os.getloadavg()},
          'cases': {}, 'binary_sha256': {}}
for name, prefix, keys in cases:
    series = []
    for batch in range(3):
        samples = {version: [] for version in ('before', 'after')}
        for version in samples:
            run(prefix, version, keys)
        for pair in range(9):
            order = ('before', 'after') if (batch + pair) % 2 == 0 else ('after', 'before')
            for version in order:
                samples[version].append(run(prefix, version, keys))
        series.append(samples)
    medians = {version: {key: statistics.median(row[key] for samples in series
        for row in samples[version]) for key in sorted(keys)} for version in ('before', 'after')}
    delta = {key: (medians['after'][key] / medians['before'][key] - 1) * 100 for key in sorted(keys)}
    per_series = [{key: (statistics.median(row[key] for row in samples['after']) /
        statistics.median(row[key] for row in samples['before']) - 1) * 100 for key in sorted(keys)} for samples in series]
    output['cases'][name] = {'samples_us': series, 'medians_us': medians,
                            'percent_change': delta, 'series_percent_change': per_series}
    for version in ('before', 'after'):
        binary = Path(f'{prefix}-{version}')
        output['binary_sha256'][str(binary)] = hashlib.sha256(binary.read_bytes()).hexdigest()
    output['end'] = {'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'load': os.getloadavg()}
    (folder / 'performance.json').write_text(json.dumps(output, indent=2) + '\n')
    print(json.dumps({'case': name, 'medians_us': medians, 'percent_change': delta,
                      'series_percent_change': per_series}), flush=True)
