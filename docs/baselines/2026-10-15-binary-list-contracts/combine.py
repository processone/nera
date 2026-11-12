"""Pool the two preserved final experiments without discarding any series."""
from pathlib import Path
import json
import statistics

folder = Path(__file__).resolve().parent
runs = [json.loads((folder / name).read_text()) for name in
        ('performance-final-first.json', 'performance-final-repeat.json')]
assert runs[0]['binary_sha256'] == runs[1]['binary_sha256']
series = [s for run in runs for s in run['cases']['binary_lists']['samples_us']]
keys = sorted(series[0]['before'][0])
medians = {v: {k: statistics.median(row[k] for s in series for row in s[v])
               for k in keys} for v in ('before', 'after')}
delta = {k: (medians['after'][k] / medians['before'][k] - 1) * 100 for k in keys}
per_series = [{k: (statistics.median(row[k] for row in s['after']) /
                  statistics.median(row[k] for row in s['before']) - 1) * 100
               for k in keys} for s in series]
output = {'start': runs[0]['start'], 'end': runs[-1]['end'],
          'binary_sha256': runs[0]['binary_sha256'],
          'cases': {'binary_lists': {'samples_us': series, 'medians_us': medians,
                                    'percent_change': delta,
                                    'series_percent_change': per_series}}}
(folder / 'performance.json').write_text(json.dumps(output, indent=2) + '\n')
print(json.dumps({'medians_us': medians, 'percent_change': delta,
                  'series_percent_change': per_series}))
