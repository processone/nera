#!/usr/bin/env python3
"""Measure single-scheduler ping/pong latency during native Base64 work.
Run on a quiet machine; do not run concurrently with builds or tests.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--nera', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--runs', type=int, default=5)
args = parser.parse_args()
compiler = args.nera.resolve()
source = Path(__file__).with_name('native_latency.erl')
env = dict(os.environ, NERA_SCHEDULERS='1')
result = {'platform': platform.platform(), 'schedulers': 1,
          'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest(),
          'source_sha256': hashlib.sha256(source.read_bytes()).hexdigest(), 'samples': []}
with tempfile.TemporaryDirectory(prefix='nera-native-latency-') as directory:
    work = Path(directory)
    for case in ['idle', 'small', 'large', 'decode_large', 'decode_spaces', 'decode_error']:
        binary = work/case
        subprocess.run([str(compiler), 'build', str(source), '-e', f'native_latency:{case}',
                        '-o', str(binary)], cwd=work, check=True, capture_output=True)
        for trial in range(args.runs+1):
            lines = subprocess.check_output([str(binary)], env=env, cwd=work,
                                            text=True, timeout=120).splitlines()
            elapsed = int(lines[0].split()[1])
            samples = sorted(map(int, lines[1:]))
            if trial == 0:
                continue  # warmup is not a recorded sample
            result['samples'].append({'case': case, 'run': trial, 'elapsed_us': elapsed,
                'count': len(samples), 'p50_us': samples[math.ceil(len(samples)*.50)-1],
                'p95_us': samples[math.ceil(len(samples)*.95)-1],
                'p99_us': samples[math.ceil(len(samples)*.99)-1], 'max_us': max(samples),
                'raw_us': samples})
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(result, indent=2)+'\n')
for row in result['samples']:
    print({k: v for k, v in row.items() if k != 'raw_us'})
