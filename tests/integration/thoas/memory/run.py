#!/usr/bin/env python3
"""Compare in-process Thoas memory observations; never vendor upstream sources."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--nera', required=True, type=Path)
    parser.add_argument('--work-dir', required=True, type=Path)
    parser.add_argument('--offline', action='store_true')
    args = parser.parse_args()
    work = args.work_dir.resolve()
    # Prevent accidentally placing third-party sources anywhere in our checkout.
    if work == ROOT or ROOT in work.parents:
        parser.error('--work-dir must be outside the Nera repository')
    work.mkdir(parents=True, exist_ok=True)
    nera = args.nera.resolve()
    report = {'status': 'failed', 'commands': []}
    env = {k: v for k, v in os.environ.items()
           if k not in ('ERL_FLAGS', 'ERL_AFLAGS', 'ERL_ZFLAGS', 'ERL_LIBS')}

    def run(label, command):
        command = list(map(str, command))
        report['commands'].append({'label': label, 'argv': command})
        result = subprocess.run(command, cwd=work, env=env, capture_output=True,
                                text=True, timeout=180)
        (work / (label + '.stdout')).write_text(result.stdout)
        (work / (label + '.stderr')).write_text(result.stderr)
        if result.returncode:
            raise ValueError(f'{label}: exit {result.returncode}; see logs in {work}')
        return result.stdout

    try:
        pin = json.loads((HERE.parent / 'differential/upstream.json').read_text())
        upstream = work / 'upstream'
        if not upstream.exists():
            if args.offline:
                raise ValueError('offline checkout missing')
            run('clone', ['git', 'clone', '--no-checkout', pin['url'], upstream])
            run('checkout', ['git', '-C', upstream, 'checkout', '--detach', pin['commit']])
        if not (upstream / '.git').exists():
            raise ValueError('independent Git checkout required')
        if run('revision', ['git', '-C', upstream, 'rev-parse', 'HEAD']).strip() != pin['commit']:
            raise ValueError('wrong upstream revision')
        if run('clean', ['git', '-C', upstream, 'status', '--porcelain', '--ignored', '--untracked-files=all']).strip():
            raise ValueError('upstream checkout is not pristine')
        sources = sorted((upstream / 'src').glob('*.erl'))
        if not sources:
            raise ValueError('upstream sources missing')
        runtime = nera.parent / 'libnera_runtime.a'
        report.update(upstream=pin, nera_sha256=sha(nera), runtime_sha256=sha(runtime),
                      driver_sha256=sha(HERE / 'thoas_memory.erl'),
                      runner_sha256=sha(Path(__file__)),
                      source_sha256={p.name: sha(p) for p in sources})
        report['otp_version'] = run('otp', ['erl', '-noshell', '-eval',
            'io:format("~s", [erlang:system_info(system_version)]), halt().']).strip()
        report['nera_version'] = run('version', [nera, '--version']).strip()
        build = work / 'build'
        build.mkdir(exist_ok=True)
        driver = HERE / 'thoas_memory.erl'
        regression = ROOT / 'tests/binary_referenced_byte_size.erl'
        run('beam-build', ['erlc', '-o', build, *sources, driver, regression])
        run('nera-build', [nera, 'build', *sources, driver, '-e', 'thoas_memory:start', '-o', build / 'native'])
        run('regression-build', [nera, 'build', regression, '-e', 'binary_referenced_byte_size:test', '-o', build / 'regression'])
        expected = ''.join(f'THOAS_MEMORY:{name}:210:{key}:{value}\n' for name, key, value in
                           [('reference', 210, 210), ('copy', 101, 102),
                            ('keys_copy', 101, 210), ('strings_copy', 210, 102)]) + 'THOAS_MEMORY_DONE:4\n'
        for engine, command in [('beam', ['erl', '-noshell', '-pa', build, '-eval', 'thoas_memory:start(), halt().']),
                                ('nera', [build / 'native'])]:
            output = run(engine, command)
            if output != expected:
                raise ValueError(f'{engine}: incomplete or unexpected memory observations')
        for engine, command in [('beam', ['erl', '-noshell', '-pa', build, '-eval', 'binary_referenced_byte_size:test(), halt().']),
                                ('nera', [build / 'regression'])]:
            if run(engine + '-regression', command) != 'ok\n':
                raise ValueError(f'{engine}: regression did not complete')
        report.update(status='passed', memory_cases=4, observations=expected.splitlines(), regression='passed on both engines')
    except (OSError, ValueError, subprocess.TimeoutExpired) as exc:
        report['error'] = str(exc)
    (work / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f'{report["status"]}: {work / "report.json"}')
    if 'error' in report:
        print(report['error'])
    return 0 if report['status'] == 'passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
