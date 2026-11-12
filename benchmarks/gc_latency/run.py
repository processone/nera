#!/usr/bin/env python3
"""Build before/after executables separately, then measure them without builds running.

Build once at each source revision using its completed release runtime + compiler:
  python3 benchmarks/gc_latency/run.py build --nera /path/to/nera --output /tmp/before
  python3 benchmarks/gc_latency/run.py build --nera /path/to/nera --output /tmp/after
Measure on an otherwise idle machine; each subprocess has exactly one scheduler:
  python3 benchmarks/gc_latency/run.py measure --before /tmp/before --after /tmp/after --output /tmp/results
"""
import argparse
import gzip
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess

HERE = Path(__file__).resolve().parent
CASES = ('large', 'huge', 'waits', 'idle')
ENV = dict(os.environ, NERA_SCHEDULERS='1')


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build(args):
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    source = out / 'gc_latency.erl'
    shutil.copy2(HERE / source.name, source)
    for case in CASES:
        p = subprocess.run([str(args.nera.resolve()), 'build', str(source), '-e',
                            'gc_latency:' + case, '-o', str(out / case)],
                           cwd=out, env=ENV, capture_output=True, text=True, timeout=120)
        (out / (case + '.build.log')).write_text(p.stdout + p.stderr)
        p.check_returncode()
    (out / 'build.json').write_text(json.dumps({
        'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=HERE, text=True).strip(),
        'source_sha256': sha(source), 'compiler_sha256': sha(args.nera),
        'binaries': {case: sha(out / case) for case in CASES},
    }, indent=2) + '\n')


def trial(exe):
    p = subprocess.run([str(exe)], cwd=exe.parent, env=ENV,
                       capture_output=True, text=True, timeout=120)
    p.check_returncode()
    lines = p.stdout.splitlines()
    assert lines[0].startswith('elapsed ') and lines[1].startswith('operations '), p.stdout[:300]
    values = sorted(map(int, lines[2:]))
    assert values and min(values) >= 0
    return {'elapsed_us': int(lines[0].split()[1]), 'operations': int(lines[1].split()[1]),
            'samples': len(values), 'p50_us': values[math.ceil(len(values)*.5)-1],
            'p99_us': values[math.ceil(len(values)*.99)-1], 'max_us': max(values)}, p.stdout


def measure(args):
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    builds = {'before': args.before.resolve(), 'after': args.after.resolve()}
    result = {'platform': platform.platform(), 'machine': platform.machine(),
              'schedulers': 1, 'warmups': 1, 'repetitions': args.repetitions,
              'source_sha256': sha(HERE / 'gc_latency.erl'),
              'builds': {name: json.loads((path / 'build.json').read_text()) for name, path in builds.items()},
              'cases': {}}
    for case in CASES:
        for name, path in builds.items():
            assert sha(path / case) == result['builds'][name]['binaries'][case]
            assert result['builds'][name]['source_sha256'] == result['source_sha256']
            trial(path / case)  # Fresh-process warmup; each measured process remains cold.
        rows = {'before': [], 'after': []}
        for i in range(args.repetitions):
            order = ['before', 'after'] if i % 2 == 0 else ['after', 'before']
            for name in order:
                row, raw = trial(builds[name] / case)
                rows[name].append(row)
                with gzip.open(out / f'{case}-{name}-{i}.txt.gz', 'wt') as f:
                    f.write(raw)
                print(case, name, i, row, flush=True)
        result['cases'][case] = {name: {
            'trials': trials,
            'median_p99_us': statistics.median(t['p99_us'] for t in trials),
            'median_max_us': statistics.median(t['max_us'] for t in trials),
            'worst_max_us': max(t['max_us'] for t in trials),
            'median_operations': statistics.median(t['operations'] for t in trials),
        } for name, trials in rows.items()}
        (out / 'results.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('build')
    p.add_argument('--nera', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p = sub.add_parser('measure')
    p.add_argument('--before', type=Path, required=True)
    p.add_argument('--after', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--repetitions', type=int, default=5)
    args = parser.parse_args()
    if args.command == 'measure' and args.repetitions < 1:
        parser.error('repetitions must be positive')
    (build if args.command == 'build' else measure)(args)
