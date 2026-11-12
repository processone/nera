#!/usr/bin/env python3
"""macOS paired small-message benchmark; no runtime edits."""
import argparse
import gzip
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import time

HERE = Path(__file__).resolve().parent
CASES = ['tuple2']

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def build(args):
    args.output.mkdir(parents=True, exist_ok=True)
    source = HERE / 'small_messages.erl'
    (args.output / source.name).write_bytes(source.read_bytes())
    for case in CASES:
        for suffix in ['', '_multi']:
            name = case + suffix
            result = subprocess.run([str(args.compiler.resolve()), 'build', source.name,
                '-e', 'small_messages:' + name, '-o', str((args.output / name).resolve())],
                cwd=args.output, capture_output=True, text=True, timeout=120)
            (args.output / (name + '.build.log')).write_text(result.stdout + result.stderr)
            result.check_returncode()
    (args.output / 'build.json').write_text(json.dumps({
        'compiler': sha(args.compiler), 'runtime': sha(args.compiler.parent / 'libnera_runtime.a'),
        'source': sha(source), 'binaries': {c+s: sha(args.output / (c+s))
            for c in CASES for s in ['', '_multi']}}, indent=2)+'\n')

def trial(binary, schedulers):
    load = sum(map(float, subprocess.check_output(['ps', '-axo', 'pcpu='], text=True).split()))
    start = time.perf_counter()
    result = subprocess.run(['/usr/bin/time', '-l', str(binary.resolve())],
        env=dict(os.environ, NERA_SCHEDULERS=str(schedulers)),
        capture_output=True, text=True, timeout=120)
    wall = time.perf_counter()-start
    result.check_returncode()
    lines = result.stdout.splitlines()
    batches = 100 if schedulers == 1 else 30
    warmup = 10 if schedulers == 1 else 5
    assert len(lines) == batches*2, result.stdout[-2000:]
    windows, samples = [], []
    for index in range(batches):
        header = lines[index*2].split()
        assert header[:2] == ['batch','20000'], header
        rtts = json.loads(lines[index*2+1])
        assert rtts and all(isinstance(x,int) and x>=0 for x in rtts)
        if index >= warmup:
            windows.append(int(header[2])); samples.extend(rtts)
    samples.sort()
    cpu = re.search(r'([0-9.]+) user\s+([0-9.]+) sys', result.stderr)
    return {'window_us': windows, 'median_batch_us': statistics.median(windows),
        'mean_batch_us': statistics.mean(windows),
        'messages_per_second': len(windows)*20000*1e6/sum(windows),
        'latency_samples': len(samples), 'p50_us': statistics.median(samples), 'p99_us': samples[math.ceil(.99*len(samples))-1],
        'max_us': samples[-1], 'wall_seconds': wall,
        'cpu_seconds': float(cpu[1])+float(cpu[2]),
        'rss_bytes': int(re.search(r'(\d+)\s+maximum resident set size',result.stderr)[1]),
        'host_cpu_percent': load}, result.stdout+result.stderr

def measure(args):
    args.output.mkdir(parents=True, exist_ok=True)
    meta = {k:json.loads((getattr(args,k)/'build.json').read_text()) for k in ['before','after']}
    assert meta['before']['compiler'] == meta['after']['compiler']
    assert meta['before']['source'] == meta['after']['source'] == sha(HERE/'small_messages.erl')
    for k,m in meta.items():
        for name,h in m['binaries'].items(): assert sha(getattr(args,k)/name)==h
    data = {'platform': platform.platform(), 'builds': meta, 'cases': args.cases,
            'schedulers': args.schedulers, 'repetitions': args.repeats, 'trials': {}}
    for schedulers in args.schedulers:
        suffix = '' if schedulers==1 else '_multi'
        for repeat in range(args.repeats):
            offset = repeat % len(args.cases)
            cases = args.cases[offset:]+args.cases[:offset]
            for case in cases:
                for version in (['before','after'] if repeat%2==0 else ['after','before']):
                    key = f'{schedulers}-{case}-{version}'
                    row,raw = trial(getattr(args,version)/(case+suffix),schedulers)
                    data['trials'].setdefault(key,[]).append(row)
                    with gzip.open(args.output/f'{key}-{repeat}.log.gz','wt') as f: f.write(raw)
                    print(key,repeat,row['median_batch_us'],row['p99_us'],flush=True)
                    (args.output/'progress.json').write_text(json.dumps(data,indent=2)+'\n')
    fields = ['median_batch_us','mean_batch_us','messages_per_second','p50_us','p99_us','max_us',
              'rss_bytes','latency_samples','cpu_seconds','host_cpu_percent']
    data['medians'] = {k:{f:statistics.median(r[f] for r in rows) for f in fields}
                       for k,rows in data['trials'].items()}
    data['paired_batch_change_percent'] = {}
    for n in args.schedulers:
        for c in args.cases:
            pairs = [100*(b['median_batch_us']/a['median_batch_us']-1) for a,b in zip(
                data['trials'][f'{n}-{c}-before'],data['trials'][f'{n}-{c}-after'])]
            data['paired_batch_change_percent'][f'{n}-{c}'] = {'all':pairs,
                'median':statistics.median(pairs),'min':min(pairs),'max':max(pairs)}
    (args.output/'results.json').write_text(json.dumps(data,indent=2)+'\n')

if __name__ == '__main__':
    p=argparse.ArgumentParser(description=__doc__); sub=p.add_subparsers(dest='mode',required=True)
    b=sub.add_parser('build'); b.add_argument('--compiler',type=Path,required=True); b.add_argument('--output',type=Path,required=True)
    m=sub.add_parser('measure')
    for name in ['before','after','output']: m.add_argument('--'+name,type=Path,required=True)
    m.add_argument('--cases', nargs='+', choices=CASES, default=CASES)
    m.add_argument('--schedulers', nargs='+', type=int, choices=[1,4], default=[1,4])
    m.add_argument('--repeats', type=int, default=7)
    a=p.parse_args(); (build if a.mode=='build' else measure)(a)
