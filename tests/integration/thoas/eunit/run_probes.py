#!/usr/bin/env python3
"""Exercise real installed OTP headers/assertions on BEAM and Nera."""
import argparse
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--nera', type=Path, required=True)
    parser.add_argument('--work-dir', type=Path, required=True)
    args = parser.parse_args()
    work, nera = args.work_dir.resolve(), args.nera.resolve()
    if work.is_relative_to(ROOT):
        parser.error('work directory must be outside the repository')
    work.mkdir(parents=True, exist_ok=True)
    report = {'status': 'failed', 'probes': []}

    def run(label, command, expected=None):
        result = subprocess.run(command, cwd=work, capture_output=True, text=True, timeout=180)
        (work / f'{label}.stdout').write_text(result.stdout)
        (work / f'{label}.stderr').write_text(result.stderr)
        if result.returncode or (expected is not None and result.stdout != expected + '\n'):
            raise RuntimeError(f'{label}: execution failed or completion marker missing')

    try:
        for module, marker in [('header_probe', 'EUNIT_HEADERS_OK'),
                               ('assertion_probe', 'OTP_ASSERTIONS_OK')]:
            source = HERE / f'{module}.erl'
            run(module + '-beam-build', ['erlc', '-o', work, source])
            run(module + '-beam', ['erl', '-noshell', '-pa', work, '-eval',
                                  f'{module}:test(), halt().'], marker)
            executable = work / module
            run(module + '-nera-build', [nera, 'build', source, '-e', f'{module}:test', '-o', executable])
            run(module + '-nera', [executable], marker)
            report['probes'].append({'module': module, 'beam': marker, 'nera': marker})
        report['status'] = 'passed'
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as exc:
        report['error'] = str(exc)
    (work / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))
    return 0 if report['status'] == 'passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
