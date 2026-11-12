#!/usr/bin/env python3
"""Run original cookie/date EUnit and PropEr on BEAM; test bodies on Nera.

All upstream sources and adapted copies stay in a separate external directory.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import subprocess
from runner import HERE, ROOT, external_workdir, sha

PROPER_COMMIT = '5640d0715d4b346676267504d8e84398e2a29f75'  # v1.5.0
SUITES = {
    'cow_cookie': {'generators': {'parse_cookie_test_': 13,
        'parse_cookie_error_test_': 1, 'parse_set_cookie_test_': 16,
        'cookie_test_': 5, 'setcookie_test_': 9, 'setcookie_failures_test_': 5,
        'setcookie_attr_failures_test_': 3},
        'direct': ['parse_cookie_max_cookies_test', 'setcookie_max_age_test']},
    'cow_date': {'generators': {'http_date_test_': 3, 'rfc2109_test_': 2,
        'rfc7231_test_': 2}, 'direct': []},
}


def adapt(original, module):
    suite = SUITES[module]
    names = re.findall(r'^(\w+_test_?)\(\) ->', original, re.M)
    if set(names) != set(suite['generators']) | set(suite['direct']):
        raise ValueError('functional inventory changed')
    source = original
    removed = []
    if module == 'cow_date':
        # Keep every EUnit body unchanged. Property execution is separate.
        source, n = re.subn(r'^day_name\(\) ->.*?(?=^http_date_test_\(\))', '', source, flags=re.M | re.S)
        if n != 1 or 'prop_http_date()' in source:
            raise ValueError('property inventory changed')
        source = source.replace('-include_lib("proper/include/proper.hrl").', '')
        def strip(match):
            block = match.group()
            if re.search(r'^\w+_test_?\(', block, re.M):
                raise ValueError('refusing to remove a functional test')
            removed.extend(re.findall(r'^(horse_\w+)\(\)', block, re.M))
            return ''
        source, n = re.subn(r'^horse_\w+\(\) ->.*?(?=^-endif\.)', strip, source, flags=re.M | re.S)
        if n != 3 or len(removed) != 9:
            raise ValueError('benchmark inventory changed')
    calls = [name+'()' for name in suite['generators']]
    direct = ', '.join('{"'+name+'", fun '+name+'/0}' for name in suite['direct'])
    checks = '\n'.join(f'    {count} = length({name}()),' for name, count in suite['generators'].items())
    count = sum(suite['generators'].values()) + len(suite['direct'])
    adapter = f'''
nera_upstream_start() ->
    {{ok, B}} = file:read_file("index.txt"),
    I = binary_to_integer(B),
{checks}
    Tests = lists:append([{', '.join(calls)}]) ++ [{direct}],
    {count} = length(Tests),
    {{_, F}} = lists:nth(I, Tests),
    F(),
    io:format("COWLIB_UPSTREAM:~w~n", [I]).
'''
    source = source.replace(f'-module({module}).', f'-module({module}).\n-define(TEST, true).\n-export([nera_upstream_start/0]).', 1)
    return source + adapter, removed


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('upstream', 'proper', 'nera', 'nera-source', 'work-dir'):
        p.add_argument('--'+name, type=Path, required=True)
    a = p.parse_args()
    upstream, proper, nera, checkout = (x.resolve() for x in (a.upstream, a.proper, a.nera, a.nera_source))
    work = external_workdir(a.work_dir)
    for other in (upstream, proper, checkout):
        if work == other or work in other.parents or other in work.parents:
            p.error('work directory must be separate from sources/dependencies')
    work.mkdir(parents=True, exist_ok=True)
    env = {k: v for k, v in os.environ.items() if k not in ('ERL_FLAGS', 'ERL_AFLAGS', 'ERL_ZFLAGS', 'ERL_LIBS')}
    env['ERL_LIBS'] = str(proper/'_build/default/lib')
    report = {'status': 'failed', 'platform': platform.platform(), 'commands': [], 'modules': {}}
    def run(cmd, label, cwd=work, timeout=180):
        try:
            r = subprocess.run(list(map(str, cmd)), cwd=cwd, env=env, capture_output=True, timeout=timeout)
            out, err, code = r.stdout, r.stderr, r.returncode
        except subprocess.TimeoutExpired as e:
            out, err, code = e.stdout or b'', e.stderr or b'', 'timeout'
        (work/(label+'.stdout')).write_bytes(out)
        (work/(label+'.stderr')).write_bytes(err)
        report['commands'].append({'argv': list(map(str, cmd)), 'label': label, 'returncode': code})
        return out.decode(errors='replace'), code
    def require(cmd, label, cwd=work):
        out, code = run(cmd, label, cwd)
        if code != 0:
            raise ValueError(f'{label}: {code}')
        return out.strip()
    try:
        pin = json.loads((HERE/'upstream.json').read_text())
        if require(['git', '-C', upstream, 'rev-parse', 'HEAD'], 'upstream-head') != pin['commit']:
            raise ValueError('wrong Cowlib revision')
        if require(['git', '-C', upstream, 'status', '--porcelain', '--ignored'], 'upstream-clean'):
            raise ValueError('dirty Cowlib checkout')
        if require(['git', '-C', proper, 'rev-parse', 'HEAD'], 'proper-head') != PROPER_COMMIT:
            raise ValueError('wrong PropEr revision')
        if require(['git', '-C', proper, 'diff', 'HEAD', '--', 'src', 'include', 'rebar.config'], 'proper-clean'):
            raise ValueError('modified PropEr source')
        if require(['git', '-C', checkout, 'status', '--porcelain'], 'nera-clean'):
            raise ValueError('Nera compiler checkout must be clean')
        report.update(upstream=pin, proper_commit=PROPER_COMMIT,
            nera_commit=require(['git', '-C', checkout, 'rev-parse', 'HEAD'], 'nera-head'),
            nera_sha256=sha(nera), runtime_sha256=sha(nera.parent/'libnera_runtime.a'),
            harness_sha256=sha(Path(__file__)),
            source_sha256={m: sha(upstream/'src'/f'{m}.erl') for m in SUITES},
            otp_version=require(['erl', '-noshell', '-eval', 'io:format("~s",[erlang:system_info(system_version)]),halt().'], 'otp'))
        beam = work/'beam'; beam.mkdir(exist_ok=True)
        require(['erlc', '-DTEST', '+{parse_transform,eunit_autoexport}', '+export_all', '-I', upstream/'include', '-o', beam,
            upstream/'src/cow_cookie.erl', upstream/'src/cow_date.erl'], 'beam-build')
        for module, suite in SUITES.items():
            count = sum(suite['generators'].values()) + len(suite['direct'])
            m = {'expected': count, 'inventory': suite, 'native': []}
            report['modules'][module] = m
            checks = ','.join(f'{n}=length({module}:{name}())' for name, n in suite['generators'].items())
            expr = checks + f',case eunit:test({module},[verbose]) of ok -> io:format("UPSTREAM_EUNIT_OK~n"),halt(0); _ -> halt(1) end.'
            out, code = run(['erl', '-noshell', '-pa', beam, '-eval', expr], module+'-beam')
            m['beam_passed'] = code == 0 and f'All {count} tests passed.' in out and 'UPSTREAM_EUNIT_OK' in out
            native = work/module; native.mkdir(exist_ok=True)
            source, removed = adapt((upstream/'src'/f'{module}.erl').read_text(), module)
            src = native/f'{module}.erl'; src.write_text(source)
            m.update(adapted_sha256=sha(src), removed_benchmarks=removed)
            sources = [src] + ([upstream/'src/cow_date.erl'] if module == 'cow_cookie' else [])
            exe = native/'test'; exe.unlink(missing_ok=True)
            _, code = run([nera, 'build', *sources, '-I', upstream/'include', '-e', module+':nera_upstream_start', '-o', exe], module+'-native-build', native, 300)
            m['native_build_returncode'] = code
            if code == 0:
                for i in range(1, count+1):
                    (native/'index.txt').write_text(str(i))
                    out, rc = run([exe], f'{module}-native-{i}', native, 15)
                    m['native'].append({'index': i, 'returncode': rc, 'passed': rc == 0 and out == f'COWLIB_UPSTREAM:{i}\n'})
            m['native_passed'] = sum(x['passed'] for x in m['native'])
            m['native_blocked'] = count if code != 0 else 0
        expr = 'case proper:quickcheck(cow_date:prop_http_date(),[{numtests,1000}]) of true -> io:format("PROPERTY_OK~n"),halt(0); _ -> halt(1) end.'
        out, code = run(['erl', '-noshell', '-pa', beam, '-eval', expr], 'date-property-beam')
        report['property'] = {'name': 'prop_http_date', 'beam_passed': code == 0 and 'PROPERTY_OK' in out,
            'trials': 1000, 'seed': 'PropEr default (time based)', 'nera': 'not executed; native PropEr runtime unsupported by this adapter'}
        if sha(nera) != report['nera_sha256'] or sha(nera.parent/'libnera_runtime.a') != report['runtime_sha256']:
            raise ValueError('compiler/runtime changed')
        report['functional_status'] = 'passed' if all(m['beam_passed'] and m['native_passed'] == m['expected'] for m in report['modules'].values()) else 'failed'
        report['status'] = 'incomplete'  # Native PropEr is not claimed, even if EUnit bodies pass.
    except (OSError, ValueError) as e:
        report['error'] = str(e)
    (work/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({k: v for k, v in report.items() if k in ('status', 'functional_status', 'error', 'property')}))
    for name, m in report['modules'].items():
        print(f'{name}: BEAM {m.get("beam_passed")}; Nera {m.get("native_passed",0)}/{m["expected"]}')
    return 0 if report['status'] == 'passed' else 1

if __name__ == '__main__':
    raise SystemExit(main())
