#!/usr/bin/env python3
"""Run project-owned cow_link URI assertions using an existing pinned runner workdir."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

from runner import external_workdir, verify_beam_modules

HERE = Path(__file__).resolve().parent

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--nera', required=True, type=Path)
    parser.add_argument('--work-dir', required=True, type=Path)
    args = parser.parse_args()
    work = external_workdir(args.work_dir)
    nera = args.nera.resolve()
    upstream = work / 'upstream'
    report = json.loads((work / 'report.json').read_text())
    pin = json.loads((HERE / 'upstream.json').read_text())
    revision = subprocess.check_output(['git', '-C', str(upstream), 'rev-parse', 'HEAD'], text=True).strip()
    if revision != pin['commit'] or report['upstream']['commit'] != revision:
        raise ValueError('upstream revision differs from the pin')
    for name, expected in report['source_sha256'].items():
        if sha(upstream / name) != expected:
            raise ValueError('upstream source changed: ' + name)
    modules = ['cow_link', 'cow_http', 'cow_http1', 'cow_http_hd', 'cow_http2',
               'cow_http_struct_hd', 'cow_cookie', 'cow_date']
    ebin = work / 'beam-source' / 'ebin'
    verify_beam_modules(ebin, modules)
    folder = work / 'uri-probe'
    folder.mkdir(exist_ok=True)
    source = folder / 'cowlib_uri_probe.erl'
    shutil.copyfile(HERE / source.name, source)
    commands = []

    def run(argv, label):
        argv = list(map(str, argv))
        result = subprocess.run(argv, cwd=folder, capture_output=True, text=True, timeout=120)
        (folder / (label + '.stdout')).write_text(result.stdout)
        (folder / (label + '.stderr')).write_text(result.stderr)
        commands.append({'argv': argv, 'label': label, 'returncode': result.returncode})
        if result.returncode:
            raise RuntimeError(label + ' failed; inspect ' + str(folder))
        return result

    run(['erlc', '-o', folder, source], 'beam-compile')
    beam = run(['erl', '-noshell', '-pa', ebin, folder, '-eval', 'ok=cowlib_uri_probe:test(),halt().'], 'beam')
    native = folder / 'native'
    run([nera, 'build', source, *[upstream / 'src' / (m + '.erl') for m in modules],
         '-I', upstream / 'include', '-e', 'cowlib_uri_probe:test', '-o', native], 'native-build')
    result = run([native], 'native')
    if beam.stdout != 'ok\n' or result.stdout != beam.stdout or result.stderr:
        raise RuntimeError('missing completion or URI assertion failure; inspect ' + str(folder))
    evidence = {'status': 'passed', 'assertions': 9, 'upstream_commit': revision,
                'nera_sha256': sha(nera), 'fixture_sha256': sha(source),
                'native_executable_sha256': sha(native), 'commands': commands,
                'beam_stdout': beam.stdout, 'native_stdout': result.stdout}
    (folder / 'report.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print('cow_link URI assertions: 9/9 BEAM and Nera; ' + str(folder / 'report.json'))

if __name__ == '__main__':
    main()
