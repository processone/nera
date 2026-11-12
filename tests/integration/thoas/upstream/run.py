#!/usr/bin/env python3
"""Execute the pinned upstream EUnit/ExUnit suites; keep all downloads outside Nera."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def verdict(eunit_ok, summary):
    # Counts belong to the pinned Thoas revision, not a moving branch.
    if summary is None or summary.get('total') != 338 or any(summary.get(k) != 0 for k in ('excluded', 'skipped')):
        return 'incomplete'
    if not eunit_ok or summary.get('failures') != 0:
        return 'failed'
    return 'passed'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def bridge(executable, directory):
    """One isolated, bounded native invocation; preserve request, reply and logs."""
    directory = Path(directory)
    try:
        proc = subprocess.run([executable], cwd=directory, capture_output=True, timeout=10)
        (directory / 'stdout').write_bytes(proc.stdout)
        (directory / 'stderr').write_bytes(proc.stderr)
        if proc.returncode != 0:
            raise ValueError(f'native exit {proc.returncode}')
        if not (directory / 'reply.etf').is_file():
            raise ValueError('missing native reply')
        status = {'status': 'completed', 'returncode': proc.returncode}
    except subprocess.TimeoutExpired as exc:
        (directory / 'stdout').write_bytes(exc.stdout or b'')
        (directory / 'stderr').write_bytes(exc.stderr or b'')
        status = {'status': 'failed', 'error': str(exc)}
    except (OSError, ValueError) as exc:
        status = {'status': 'failed', 'error': str(exc)}
    (directory / 'transport.json').write_text(json.dumps(status) + '\n')
    if status['status'] != 'completed':
        print(status['error'], file=sys.stderr)
        return 1
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--work-dir', type=Path, default=Path(tempfile.gettempdir()) / 'nera-thoas-upstream-validation')
    parser.add_argument('--nera', type=Path, default=ROOT / 'target/release/nera')
    parser.add_argument('--offline', action='store_true', help='reuse the verified checkout and downloaded Mix dependencies')
    args = parser.parse_args()
    work = args.work_dir.resolve()
    if work == ROOT or ROOT in work.parents:
        parser.error('the work directory must be outside the Nera repository')
    work.mkdir(parents=True, exist_ok=True)
    env = {k: v for k, v in os.environ.items() if k not in ('ERL_FLAGS', 'ERL_AFLAGS', 'ERL_ZFLAGS', 'ERL_LIBS')}
    env['MIX_ENV'] = 'test'
    report = {'status': 'incomplete', 'platform': platform.platform(), 'started_unix': time.time(), 'commands': [], 'engines': {}}

    def run(cmd, label, cwd=work, timeout=180, allow_failure=False, extra=None):
        cmd = list(map(str, cmd))
        report['commands'].append({'label': label, 'argv': cmd, 'cwd': str(cwd)})
        try:
            p = subprocess.run(cmd, cwd=cwd, env=env | (extra or {}), capture_output=True, timeout=timeout)
            out, err, rc = p.stdout, p.stderr, p.returncode
        except subprocess.TimeoutExpired as exc:
            out, err, rc = exc.stdout or b'', exc.stderr or b'', 'timeout'
        (work / (label + '.stdout')).write_bytes(out)
        (work / (label + '.stderr')).write_bytes(err)
        report['commands'][-1]['returncode'] = rc
        if rc != 0 and not allow_failure:
            raise ValueError(f'{label}: {rc}; see logs in {work}')
        return out.decode('utf-8', errors='replace'), rc

    try:
        pin = json.loads((HERE.parent / 'differential/upstream.json').read_text())
        upstream = work / 'upstream'
        if not upstream.exists():
            if args.offline:
                raise ValueError('offline checkout missing')
            run(['git', 'clone', '--no-checkout', pin['url'], upstream], 'clone')
            run(['git', '-C', upstream, 'checkout', '--detach', pin['commit']], 'checkout')
        if not (upstream / '.git').exists():
            raise ValueError('source is not an independent checkout')
        head, _ = run(['git', '-C', upstream, 'rev-parse', 'HEAD'], 'source-head')
        status, _ = run(['git', '-C', upstream, 'status', '--porcelain', '--untracked-files=all', '--ignored'], 'source-status')
        if head.strip() != pin['commit'] or status.strip():
            raise ValueError('upstream checkout must be clean at the pinned revision')
        report.update(upstream=pin, nera_sha256=sha(args.nera.resolve()),
                      harness_sha256={p.name: sha(p) for p in HERE.iterdir() if p.is_file()},
                      source_sha256={str(p.relative_to(upstream)): sha(p) for p in upstream.rglob('*') if p.is_file() and '.git' not in p.relative_to(upstream).parts})
        report['nera_source_commit'] = run(['git', 'rev-parse', 'HEAD'], 'nera-head', ROOT)[0].strip()
        report['nera_worktree_status'] = run(['git', 'status', '--porcelain'], 'nera-status', ROOT)[0]
        report['property_config'] = {'seed': 0, 'max_runs': 100, 'max_cases': 1, 'test_timeout_ms': 300000}
        report['elixir_version'] = run(['elixir', '--version'], 'elixir-version')[0]
        report['otp_version'] = run(['erl', '-noshell', '-eval', 'R=erlang:system_info(otp_release),{ok,V}=file:read_file(filename:join([code:root_dir(),"releases",R,"OTP_VERSION"])),io:format("~s",[V]),halt().'], 'otp-version')[0].strip()
        eunit_path = run(['erl', '-noshell', '-eval', 'io:format("~s",[code:lib_dir(eunit,ebin)]),halt().'], 'eunit-path')[0].strip()
        build = work / 'build'
        build.mkdir(exist_ok=True)
        source = build / 'source'
        # Separate Mix workspace: compilation never dirties the verified checkout.
        if source.exists():
            # Preserve only previously downloaded, lock-checked Hex dependencies.
            deps = source / 'mix_tests/deps'
            cache = work / 'mix-deps-cache'
            if cache.exists():
                shutil.rmtree(cache)
            if deps.exists():
                shutil.move(deps, cache)
            shutil.rmtree(source)
        shutil.copytree(upstream, source, ignore=shutil.ignore_patterns('.git'))
        cache = work / 'mix-deps-cache'
        if cache.exists():
            shutil.move(cache, source / 'mix_tests/deps')
        mix = source / 'mix_tests'
        if not args.offline:
            run(['mix', 'deps.get'], 'mix-deps', mix)
        run(['mix', 'deps.compile'], 'mix-deps-compile', mix)
        report['test_dependency_sha256'] = {str(p.relative_to(mix / 'deps')): sha(p) for p in (mix / 'deps').rglob('*') if p.is_file()}
        # deps.get must not silently update the upstream lock.
        if sha(mix / 'mix.lock') != sha(upstream / 'mix_tests/mix.lock'):
            raise ValueError('Mix changed the upstream dependency lock')
        beam = build / 'beam'
        beam.mkdir(exist_ok=True)
        sources = sorted((upstream / 'src').glob('*.erl'))
        tests = sorted((upstream / 'test').glob('*.erl'))
        run(['erlc', '-o', beam, *sources, *tests], 'beam-build', build)
        # Preserve the direct compilation failure separately from adapted execution.
        _, direct_rc = run([args.nera.resolve(), 'build', *sources, *tests, '-e', 'thoas_encode_test:test', '-o', build / 'direct-eunit'], 'nera-direct-eunit', build, allow_failure=True)
        report['direct_eunit_compile_returncode'] = direct_rc
        native = build / 'native'
        run([args.nera.resolve(), 'build', *sources, HERE / 'native_bridge.erl', '-e', 'native_bridge:start', '-o', native], 'nera-build', build)
        if sha(args.nera.resolve()) != report['nera_sha256']:
            raise ValueError('compiler changed during the build')
        report['native_sha256'] = sha(native)
        link_log = (work / 'nera-build.stderr').read_text()
        runtime = re.search(r'Linking with runtime: (.+)', link_log)
        if runtime:
            runtime_path = Path(runtime.group(1).strip())
            report['runtime'] = {'path': str(runtime_path), 'sha256': sha(runtime_path)}
        for engine in ('beam', 'nera'):
            calls = work / (engine + '-calls')
            if calls.exists():
                shutil.rmtree(calls)
            calls.mkdir()
            results = work / (engine + '-tests.tsv')
            results.unlink(missing_ok=True)
            print(f'Running upstream suites: {engine}', flush=True)
            out, rc = run(['mix', 'run', '--no-start', '--no-compile', '--no-deps-check', HERE / 'launch.exs'], engine, mix,
                          timeout=1800, allow_failure=True, extra={
                              'THOAS_EUNIT': eunit_path, 'THOAS_BEAM': str(beam), 'THOAS_ENGINE': engine, 'THOAS_CALLS': str(calls),
                              'THOAS_NATIVE': str(native), 'THOAS_PYTHON': sys.executable,
                              'THOAS_RUNNER': str(Path(__file__).resolve()), 'THOAS_TEST_RESULTS': str(results)})
            match = re.search(r'^UPSTREAM_EXUNIT:(\d+):(\d+):(\d+):(\d+)$', out, re.M)
            stats = dict(zip(('total', 'failures', 'excluded', 'skipped'), map(int, match.groups()))) if match else None
            eunit_ok = 'UPSTREAM_EUNIT:ok' in out and 'All 36 tests passed.' in out
            state = verdict(eunit_ok, stats)
            if rc != 0 and state == 'passed':
                state = 'incomplete'
            report['engines'][engine] = {'status': state, 'eunit_all_36_passed': eunit_ok, 'exunit': stats, 'returncode': rc,
                'native_calls': len(list(calls.glob('*/request.etf'))),
                'transport_failures': [str(p.parent.relative_to(work)) for p in calls.glob('*/transport.json') if json.loads(p.read_text())['status'] != 'completed']}
            if results.exists():
                entries = [line.split('\t', 2) for line in results.read_text().splitlines()]
                report['engines'][engine]['failed_tests'] = [{'module': m, 'test': t, 'detail': r} for m, t, r in entries if r != 'nil']
                report['engines'][engine]['reported_tests'] = len(entries)
                if stats and len(entries) != stats['total']:
                    report['engines'][engine]['status'] = 'incomplete'
            else:
                report['engines'][engine]['status'] = 'incomplete'
            print(f'{engine}: {state} {stats}', flush=True)
        report['status'] = 'passed' if all(x['status'] == 'passed' for x in report['engines'].values()) else 'failed'
        report['limitations'] = ['ExUnit/EUnit and StreamData execute on BEAM; only Thoas executes in Nera.',
            'ETF transport cannot preserve sub-binary sharing; the copying-strings test requires an in-process native probe.',
            'Property seed 0, 100 runs, one test at a time; no exhaustive proof or platform matrix.']
    except (OSError, ValueError) as exc:
        report['error'] = str(exc)
        print(str(exc), file=sys.stderr)
    finally:
        (work / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(f'Report: {work / "report.json"}')
    return 0 if report['status'] == 'passed' else 1


if __name__ == '__main__':
    if len(sys.argv) == 4 and sys.argv[1] == '--bridge':
        sys.exit(bridge(sys.argv[2], sys.argv[3]))
    sys.exit(main())
