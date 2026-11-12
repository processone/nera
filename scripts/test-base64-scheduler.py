#!/usr/bin/env python3
"""Check compiled Base64 progress with exactly one scheduler, also on BEAM."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--nera', type=Path, default=ROOT/'target/release/nera')
args = parser.parse_args()
compiler = args.nera.resolve()
env = dict(os.environ, NERA_SCHEDULERS='1')

def run(command, cwd, runtime_env=env):
    result = subprocess.run(list(map(str, command)), cwd=cwd, env=runtime_env,
                            capture_output=True, text=True, timeout=60)
    if result.returncode:
        raise RuntimeError(f'{command}: {result.returncode}\n{result.stdout}\n{result.stderr}')
    return result.stdout

with tempfile.TemporaryDirectory(prefix='nera-base64-scheduler-') as directory:
    work = Path(directory)
    source = work/'base64_scheduler_fairness.erl'
    source.write_bytes((ROOT/'tests'/source.name).read_bytes())
    run(['erlc', '-o', work, source], work)
    assert run(['erl', '+S', '1:1', '-noshell', '-pa', work, '-eval',
                'base64_scheduler_fairness:single_scheduler(), halt().'], work) == 'ok\n'
    print('BEAM +S 1: observer progresses during small repeated and large encodings', flush=True)
    native = work/'native'
    run([compiler, 'build', source, '-e', 'base64_scheduler_fairness:single_scheduler', '-o', native], work)
    for _ in range(5):
        assert run([native], work) == 'ok\n'
    print('Nera NERA_SCHEDULERS=1: same progress assertions pass in 5 runs', flush=True)
    source = work/'base64_large_decode.erl'
    source.write_bytes((ROOT/'tests'/source.name).read_bytes())
    run(['erlc', '-o', work, source], work)
    assert run(['erl', '+S', '1:1', '-noshell', '-pa', work, '-eval',
                'base64_large_decode:single_scheduler(), halt().'], work) == 'ok\n'
    run([compiler, 'build', source, '-e', 'base64_large_decode:single_scheduler', '-o', native], work)
    for _ in range(5):
        assert run([native], work) == 'ok\n'
    print('BEAM +S 1 and Nera (5 runs): decoder yields on data, whitespace and late errors')
    source = work/'base64_list_fairness.erl'
    source.write_bytes((ROOT/'tests'/source.name).read_bytes())
    run(['erlc', '-o', work, source], work)
    assert run(['erl', '+S', '1:1', '-noshell', '-pa', work, '-eval',
                'base64_list_fairness:single_scheduler(), halt().'], work) == 'ok\n'
    run([compiler, 'build', source, '-e', 'base64_list_fairness:single_scheduler', '-o', native], work)
    for _ in range(5):
        assert run([native], work) == 'ok\n'
    print('BEAM +S 1 and Nera (5 runs): list decoder yields on data, whitespace and late errors')
    # Bad configuration must fail before executing the entry point.
    rejected = subprocess.run([native], cwd=work, env=dict(env, NERA_SCHEDULERS='0'),
                              capture_output=True, text=True, timeout=10)
    assert rejected.returncode == 2 and 'NERA_SCHEDULERS' in rejected.stderr, rejected
    print('Invalid scheduler count rejected before execution')
