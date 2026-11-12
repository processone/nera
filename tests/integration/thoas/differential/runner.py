#!/usr/bin/env python3
"""Pinned, fail-closed BEAM/Nera differential test; Python standard library only."""
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
import time
from decimal import Decimal

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def etf(data):
    """Read only the ETF types emitted by this corpus; reject everything else.

    Preserve Erlang types, binary bytes and IEEE float bits. Map entry order and
    alternative ETF encodings (small integers, atoms, strings) are not semantic.
    """
    pos = 0

    def take(n):
        nonlocal pos
        if n < 0 or pos + n > len(data):
            raise ValueError('truncated ETF')
        value = data[pos:pos + n]
        pos += n
        return value

    def uint(n):
        return int.from_bytes(take(n), 'big')

    def term():
        tag = uint(1)
        if tag == 97:
            return ('integer', uint(1))
        if tag == 98:
            return ('integer', int.from_bytes(take(4), 'big', signed=True))
        if tag in (110, 111):
            n = uint(1 if tag == 110 else 4)
            sign = uint(1)
            if sign not in (0, 1):
                raise ValueError('invalid big integer sign')
            return ('integer', int.from_bytes(take(n), 'little') * (-1 if sign else 1))
        if tag == 70:
            return ('float', take(8).hex())
        if tag in (100, 115, 118, 119):
            n = uint(1 if tag in (115, 119) else 2)
            return ('atom', take(n).decode('utf-8' if tag in (118, 119) else 'latin1'))
        if tag == 109:
            return ('binary', take(uint(4)).hex())
        if tag in (104, 105):
            return ('tuple', tuple(term() for _ in range(uint(1 if tag == 104 else 4))))
        if tag == 106:
            return ('list', ())
        if tag == 107:
            return ('list', tuple(('integer', b) for b in take(uint(2))))
        if tag == 108:
            items = tuple(term() for _ in range(uint(4)))
            tail = term()
            if tail[0] == 'list':
                return ('list', items + tail[1])
            return ('improper_list', items, tail)
        if tag == 116:
            pairs = [(term(), term()) for _ in range(uint(4))]
            return ('map', tuple(sorted(pairs, key=repr)))
        raise ValueError(f'unsupported ETF tag {tag}')

    if uint(1) != 131:
        raise ValueError('missing ETF version')
    result = term()
    if pos != len(data):
        raise ValueError('trailing ETF data')
    return result


def read_output(stdout, ids):
    results = {}
    done = False
    for line in stdout.splitlines():
        if done:
            raise ValueError('output after completion')
        if line.startswith('THOAS_CASE:'):
            _, case_id, payload = line.split(':', 2)
            if case_id not in ids or case_id in results:
                raise ValueError(f'unknown or duplicate case {case_id}')
            results[case_id] = etf(bytes(json.loads(payload)))
        elif line == f'THOAS_DONE:{len(ids)}':
            done = True
        else:
            raise ValueError(f'unexpected output: {line[:160]}')
    if not done:
        raise ValueError('missing completion marker')
    if set(results) != set(ids):
        raise ValueError('missing cases: ' + ', '.join(sorted(set(ids) - set(results))))
    return results


def atom(name):
    return ('atom', name)


def unpack(value):
    if value[0] != 'tuple':
        raise ValueError(f'expected tuple, got {value!r}')
    return value[1]


def classification(value, operation):
    outer = unpack(value)
    if outer[0] == atom('exception') and len(outer) == 3:
        return 'exception'
    if outer[0] != atom('return') or len(outer) != 2:
        raise ValueError('invalid outcome')
    if operation == 'decode':
        result = unpack(outer[1])
        if len(result) == 2 and result[0] in (atom('ok'), atom('error')):
            return 'ok' if result[0] == atom('ok') else 'error'
        raise ValueError('invalid decode outcome')
    if outer[1][0] != 'binary':
        raise ValueError('encoder did not return binary')
    return 'ok'


def json_value(binary):
    def reject_constant(value):
        raise ValueError(f'non-JSON constant {value}')

    # Object order is unspecified; keep duplicate members and distinguish
    # objects from arrays. Decimal ignores valid differences in number spelling.
    value = json.loads(bytes.fromhex(binary[1]).decode('utf-8'),
                      parse_int=Decimal, parse_float=Decimal,
                      parse_constant=reject_constant,
                      object_pairs_hook=lambda pairs: ('object', sorted(pairs, key=lambda pair: pair[0])))

    def typed(value):
        if value is None:
            return ('null',)
        if isinstance(value, bool):
            return ('boolean', value)
        if isinstance(value, Decimal):
            return ('number', value)
        if isinstance(value, str):
            value.encode('utf-8')  # reject lone Unicode surrogates
            return ('string', value)
        if isinstance(value, list):
            return ('array', tuple(typed(v) for v in value))
        return ('object', tuple((typed(k), typed(v)) for k, v in value[1]))

    return typed(value)


def compare(beam, nera, cases):
    failures = []
    for case in cases:
        key = case['id']
        a, b = beam[key], nera[key]
        try:
            for name, value in [('BEAM', a), ('Nera', b)]:
                if 'expect' in case and classification(value, case['operation']) != case['expect']:
                    raise ValueError(f'{name}: expected {case["expect"]}')
            if case['operation'] == 'encode' and classification(a, 'encode') == 'ok' and classification(b, 'encode') == 'ok':
                equal = json_value(unpack(a)[1]) == json_value(unpack(b)[1])
            else:
                equal = a == b
            if not equal:
                raise ValueError('different results')
        except (ValueError, UnicodeError) as exc:
            failures.append(f'{key}: {exc}\n  BEAM {a!r}\n  Nera {b!r}')
    if failures:
        raise ValueError('\n'.join(failures))


def source(cases):
    calls = []
    for case in cases:
        calls.append(f'    run({case["id"]}, fun() -> {case["expression"]} end)')
    return '''-module(thoas_differential).
-export([start/0]).
start() ->
''' + ',\n'.join(calls) + f''',
    io:format("THOAS_DONE:{len(cases)}~n"),
    ok.
run(Id, Fun) ->
    Result = try Fun() of Value -> {{return, Value}}
             catch Class:Reason -> {{exception, Class, Reason}} end,
    io:format("THOAS_CASE:~s:~w~n", [atom_to_list(Id), binary_to_list(term_to_binary(Result))]).
'''


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--nera', type=Path, default=ROOT / 'target/release/nera')
    parser.add_argument('--work-dir', type=Path, default=ROOT / 'target/thoas-differential')
    parser.add_argument('--case', action='append', default=[], help='run only this case ID (repeatable)')
    parser.add_argument('--offline', action='store_true', help='require an existing verified source checkout')
    parser.add_argument('--timeout', type=float, default=60, help='seconds per process (build: at least 180)')
    args = parser.parse_args()
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    nera = args.nera.resolve()
    for name in ('beam.json', 'nera.json'):
        (work / name).unlink(missing_ok=True)
    report = {'status': 'failed', 'started_unix': time.time(), 'platform': platform.platform(), 'python': sys.version, 'commands': []}
    env = {k: v for k, v in os.environ.items() if k not in ('ERL_FLAGS', 'ERL_AFLAGS', 'ERL_ZFLAGS', 'ERL_LIBS')}

    def run(cmd, label, cwd=work, timeout=None):
        cmd = [str(x) for x in cmd]
        report['commands'].append({'label': label, 'argv': cmd, 'cwd': str(cwd)})
        try:
            proc = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, timeout=timeout or args.timeout)
            out, err = proc.stdout, proc.stderr
        except subprocess.TimeoutExpired as exc:
            (work / f'{label}.stdout').write_bytes(exc.stdout or b'')
            (work / f'{label}.stderr').write_bytes(exc.stderr or b'')
            raise ValueError(f'{label}: timeout') from exc
        (work / f'{label}.stdout').write_bytes(out)
        (work / f'{label}.stderr').write_bytes(err)
        if proc.returncode:
            if label in ('beam', 'nera'):
                completed = re.findall(rb'^THOAS_CASE:([^:]+):', out, re.MULTILINE)
                report['failed_engine'] = label
                report['completed_before_failure'] = [i.decode('ascii') for i in completed]
                report['next_case'] = ids[len(completed)] if len(completed) < len(ids) else None
            raise ValueError(f'{label}: exit {proc.returncode}; see {work / (label + ".stderr")}')
        return out.decode('utf-8')

    try:
        pin = json.loads((HERE / 'upstream.json').read_text())
        cases = json.loads((HERE / 'corpus.json').read_text())
        if args.case:
            missing = set(args.case) - {c['id'] for c in cases}
            if missing:
                raise ValueError(f'unknown case IDs: {sorted(missing)}')
            cases = [c for c in cases if c['id'] in args.case]
        ids = [c['id'] for c in cases]
        if not ids or len(set(ids)) != len(ids) or any(not re.fullmatch('[a-z][a-z0-9_]*', i) for i in ids):
            raise ValueError('empty corpus or invalid/duplicate IDs')
        upstream = work / 'upstream'
        if not upstream.exists():
            if args.offline:
                raise ValueError('offline source checkout missing')
            run(['git', 'clone', '--no-checkout', pin['url'], upstream], 'fetch', timeout=180)
            run(['git', '-C', upstream, 'checkout', '--detach', pin['commit']], 'checkout')
        if not (upstream / '.git').exists():
            raise ValueError('upstream is not an independent Git checkout')
        actual = run(['git', '-C', upstream, 'rev-parse', 'HEAD'], 'upstream-head').strip()
        if actual != pin['commit']:
            raise ValueError(f'wrong upstream revision: {actual}')
        if run(['git', '-C', upstream, 'status', '--porcelain', '--untracked-files=all', '--ignored'], 'upstream-status').strip():
            raise ValueError('upstream checkout is modified or contains extra files')
        report.update(upstream=pin, corpus_sha256=sha(HERE / 'corpus.json'), cases=len(cases), case_ids=ids,
                      nera_sha256=sha(nera), runner_sha256=sha(Path(__file__)))
        report['nera_version'] = run([nera, '--version'], 'nera-version').strip()
        report['nera_source_commit'] = run(['git', 'rev-parse', 'HEAD'], 'nera-head', ROOT).strip()
        report['nera_worktree_status'] = run(['git', 'status', '--porcelain'], 'nera-status', ROOT)
        report['otp_version'] = run(['erl', '-noshell', '-eval', 'Release = erlang:system_info(otp_release), {ok, Version} = file:read_file(filename:join([code:root_dir(), "releases", Release, "OTP_VERSION"])), io:format("~s~n~s", [Version, erlang:system_info(system_version)]), halt().'], 'otp-version').strip()
        report['source_sha256'] = {p.name: sha(p) for p in sorted((upstream / 'src').glob('*.erl'))}
        build = work / 'build'
        if build.exists():
            shutil.rmtree(build)
        build.mkdir()
        driver = build / 'thoas_differential.erl'
        driver.write_text(source(cases))
        sources = sorted((upstream / 'src').glob('*.erl'))
        if not sources:
            raise ValueError('no Thoas sources')
        run(['erlc', '-o', build, *sources, driver], 'beam-build', build, max(180, args.timeout))
        run([nera, 'build', *sources, driver, '-e', 'thoas_differential:start', '-o', build / 'native'], 'nera-build', build, max(180, args.timeout))
        results = {}
        for engine, cmd in [('beam', ['erl', '-noshell', '-pa', build, '-eval', 'thoas_differential:start(), halt().']), ('nera', [build / 'native'])]:
            results[engine] = read_output(run(cmd, engine, build), ids)
            (work / f'{engine}.json').write_text(json.dumps(results[engine], indent=2) + '\n')
            if engine == 'beam':
                compare(results['beam'], results['beam'], cases)
                report['beam_reference'] = 'passed'
        compare(results['beam'], results['nera'], cases)
        report['status'] = 'passed'
        report['groups'] = {g: sum(c['group'] == g for c in cases) for g in sorted({c['group'] for c in cases})}
        print(f'Thoas {pin["version"]}: {len(cases)} cases agree on BEAM and Nera')
    except (ValueError, OSError, RecursionError) as exc:
        report['error'] = str(exc)
        print(str(exc), file=sys.stderr)
    finally:
        (work / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(f'Report: {work / "report.json"}')
    return 0 if report['status'] == 'passed' else 1


if __name__ == '__main__':
    sys.exit(main())
