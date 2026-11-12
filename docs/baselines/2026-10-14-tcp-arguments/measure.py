"""Three series of nine alternating native before/after pairs."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess

folder = Path(__file__).resolve().parent
binaries = {name: Path(f'/tmp/nera-recv-{name}') for name in ('before', 'after')}

def run(name):
    result = subprocess.run([str(binaries[name])], check=True, capture_output=True,
        text=True, timeout=30, env={**os.environ, 'NERA_SCHEDULERS': '1'})
    rows = {size: int(elapsed) for size, elapsed in
        re.findall(r'^recv_(\d+) (\d+)$', result.stdout, re.M)}
    assert set(rows) == {'1', '64', '4096', '1048576'}, result.stdout
    return rows

series = []
start = {'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'load': os.getloadavg()}
for index in range(3):
    samples = {name: [] for name in binaries}
    for name in binaries:
        run(name)
    for pair in range(9):
        order = ('before', 'after') if (pair + index) % 2 == 0 else ('after', 'before')
        for name in order:
            samples[name].append(run(name))
    series.append(samples)
    print(f'Series {index + 1} complete', flush=True)
keys = series[0]['before'][0].keys()
pooled = {name: {size: statistics.median(row[size] for batch in series
    for row in batch[name]) for size in keys} for name in binaries}
deltas = {size: (pooled['after'][size] / pooled['before'][size] - 1) * 100 for size in keys}
per_series = [{size: (statistics.median(row[size] for row in batch['after']) /
    statistics.median(row[size] for row in batch['before']) - 1) * 100 for size in keys} for batch in series]
output = {'start': start, 'end': {'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'load': os.getloadavg()}, 'samples_us': series, 'medians_us': pooled,
    'percent_change': deltas, 'series_percent_change': per_series,
    'binary_sha256': {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in binaries.items()}}
(folder / 'performance.json').write_text(json.dumps(output, indent=2) + '\n')
print(json.dumps({'medians_us': pooled, 'percent_change': deltas, 'series_percent_change': per_series}, indent=2))
