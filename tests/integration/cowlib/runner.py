#!/usr/bin/env python3
"""Compare pinned external Cowlib modules on BEAM and Nera, one process per case."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
# Reuse our typed ETF decoder; no third-party code is copied.
ETF_SOURCE = HERE.parent / 'thoas/differential/runner.py'
_spec = importlib.util.spec_from_file_location('thoas_etf', ETF_SOURCE)
_etf_module = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_etf_module)
etf = _etf_module.etf
DEFAULT_MODULES = ('cow_qs', 'cow_cookie', 'cow_date', 'cow_base64url')
MODULES = DEFAULT_MODULES + ('cow_http', 'cow_http_hd')
# Complete runtime dependency closure for the pinned cow_http module.
HTTP_DEPENDENCIES = ('cow_http1', 'cow_http_hd', 'cow_cookie', 'cow_date',
                     'cow_http2', 'cow_http_struct_hd', 'cow_link')
# Pinned Cowlib releases. Earlier campaigns use 2.20.0; the cookie/date
# suites also run on 2.22.0, the release examples/cowlib_demo builds.
PIN_FILES = {'2.20.0': 'upstream.json', '2.22.0': 'upstream-2.22.0.json'}
DEFAULT_PIN = '2.20.0'


def load_pin(version=DEFAULT_PIN):
    pin = json.loads((HERE / PIN_FILES[version]).read_text())
    if pin.get('version') != version or not re.fullmatch(r'[0-9a-f]{40}', str(pin.get('commit', ''))):
        raise ValueError(f'invalid Cowlib pin file for {version}')
    return pin


def verify_beam_modules(ebin, modules):
    missing = sorted(module for module in set(modules)
                     if not (ebin / (module + '.beam')).is_file())
    if missing:
        raise ValueError('BEAM build did not produce modules: ' + ', '.join(missing))


def external_workdir(path):
    path = path.resolve()
    if path == ROOT or ROOT in path.parents or path in ROOT.parents:
        raise ValueError('work directory must be outside the repository, not an ancestor')
    return path


def parse_output(stdout, case_id, returncode):
    if returncode != 0:
        raise ValueError(f'exit {returncode}')
    lines = stdout.splitlines()
    prefix = f'COWLIB_CASE:{case_id}:'
    if len(lines) != 2 or not lines[0].startswith(prefix) or lines[1] != f'COWLIB_DONE:{case_id}':
        raise ValueError('missing, duplicate or unexpected result/completion marker')
    data = json.loads(lines[0][len(prefix):])
    if not isinstance(data, list) or any(type(x) is not int or not 0 <= x <= 255 for x in data):
        raise ValueError('invalid ETF bytes')
    return etf(bytes(data))


def cookie_attributes(value):
    """Compare one Set-Cookie value, preserving all bytes except attribute order.

    Scoped to our builder cases: not a general HTTP parser. Reject duplicate
    names because their relative order may affect which value a client uses.
    Never use a dict/set that could silently discard duplicate attributes.
    """
    if value[0] != 'binary':
        raise ValueError('expected Set-Cookie binary')
    wire = bytes.fromhex(value[1])
    parts = wire.split(b'; ')
    if (b'=' not in parts[0] or any(not part or b';' in part for part in parts)
            or b'\r' in wire or b'\n' in wire):
        raise ValueError('invalid Set-Cookie builder output')
    attributes = parts[1:]
    names = [part.split(b'=', 1)[0].lower() for part in attributes]
    if len(names) != len(set(names)):
        raise ValueError('duplicate Set-Cookie attribute')
    return parts[0], sorted(attributes)


def request_pseudo_headers(value):
    """Compare only unordered request pseudo fields in a leading prefix.

    Regular fields retain exact bytes, ordering and repeated occurrences.
    This is scoped to merge_pseudo_headers builder cases, not a general parser.
    """
    if value[0] != 'list':
        raise ValueError('expected proper header list')
    pseudo, regular = [], []
    names = set()
    for entry in value[1]:
        if entry[0] != 'tuple' or len(entry[1]) != 2:
            raise ValueError('expected header pair')
        name, field = entry[1]
        if name[0] != 'binary' or field[0] != 'binary':
            raise ValueError('expected binary header name and value')
        key, payload = bytes.fromhex(name[1]), bytes.fromhex(field[1])
        if not key:
            raise ValueError('empty header name')
        if key.startswith(b':'):
            if regular:
                raise ValueError('pseudo-header after regular header')
            if key not in (b':method', b':scheme', b':authority', b':path', b':protocol'):
                raise ValueError('unknown request pseudo-header')
            if key in names:
                raise ValueError('duplicate pseudo-header')
            names.add(key)
            pseudo.append((key, payload))
        else:
            regular.append((key, payload))
    return sorted(pseudo), regular


def compare_outcome(beam, nera, expected, comparison="exact"):
    for value in (beam, nera):
        if value[0] != 'tuple' or not value[1]:
            raise ValueError('invalid outcome envelope')
        items = value[1]
        if items[0] != ('atom', expected) or len(items) != (3 if expected == 'exception' else 2):
            raise ValueError(f'expected {expected}, got {value!r}')
    if comparison == 'set_cookie_attributes':
        if expected != 'return':
            raise ValueError('Set-Cookie comparison requires a return value')
        beam, nera = (cookie_attributes(value[1][1]) for value in (beam, nera))
    elif comparison == 'request_pseudo_headers':
        if expected != 'return':
            raise ValueError('pseudo-header comparison requires a return value')
        beam, nera = (request_pseudo_headers(value[1][1]) for value in (beam, nera))
    elif comparison != 'exact':
        raise ValueError(f'unknown comparison: {comparison}')
    if beam != nera:
        raise ValueError('different result or exception class/reason')


def source(cases):
    clauses = ';\n'.join(f'call({i}) -> {c["expression"]}' for i,c in enumerate(cases))
    names = ';\n'.join(f'name({i}) -> "{c["id"]}"' for i,c in enumerate(cases))
    return '''-module(cowlib_probe).
-export([start/0]).
start() ->
    {ok, B} = file:read_file("case.txt"),
    I = binary_to_integer(B),
    Result = try call(I) of V -> {return, V}
             catch C:R -> {exception, C, R} end,
    Id = name(I),
    io:format("COWLIB_CASE:~s:~w~n", [Id, binary_to_list(term_to_binary(Result))]),
    io:format("COWLIB_DONE:~s~n", [Id]).
''' + clauses + '.\n' + names + '.\n'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--nera', required=True, type=Path)
    p.add_argument('--work-dir', required=True, type=Path)
    p.add_argument('--offline', action='store_true')
    p.add_argument('--module', choices=MODULES, action='append',
                   help='repeat to select modules; default: validated four-module core')
    p.add_argument('--timeout', type=float, default=15)
    a = p.parse_args()
    work = external_workdir(a.work_dir)
    work.mkdir(parents=True, exist_ok=True)
    nera = a.nera.resolve()
    env = {k:v for k,v in os.environ.items() if k not in ('ERL_FLAGS','ERL_AFLAGS','ERL_ZFLAGS','ERL_LIBS')}
    report = {'status':'failed', 'platform':platform.platform(), 'commands':[], 'modules':{},
              'scope':'project-owned differential corpus; not upstream EUnit/PropEr suites'}

    def run(cmd, label, cwd=work, timeout=180):
        cmd = list(map(str,cmd))
        try:
            r = subprocess.run(cmd,cwd=cwd,env=env,capture_output=True,timeout=timeout)
            out,err,rc = r.stdout,r.stderr,r.returncode
        except subprocess.TimeoutExpired as e:
            out,err,rc = e.stdout or b'',e.stderr or b'','timeout'
        (work/(label+'.stdout')).write_bytes(out)
        (work/(label+'.stderr')).write_bytes(err)
        report['commands'].append({'argv':cmd,'label':label,'returncode':rc})
        return out.decode('utf-8',errors='replace'),rc

    def required(cmd,label,cwd=work,timeout=180):
        out,rc = run(cmd,label,cwd,timeout)
        if rc != 0: raise ValueError(f'{label}: {rc}; see external logs')
        return out

    try:
        pin = json.loads((HERE/'upstream.json').read_text())
        cases = json.loads((HERE/'corpus.json').read_text())
        if len({c['id'] for c in cases}) != len(cases): raise ValueError('duplicate case IDs')
        for c in cases:
            if not re.fullmatch('[a-z][a-z0-9_]*',c['id']) or c['module'] not in MODULES or c['expect'] not in ('return','exception'):
                raise ValueError('invalid corpus metadata')
        upstream = work/'upstream'
        if not upstream.exists():
            if a.offline: raise ValueError('offline checkout missing')
            required(['git','clone','--no-checkout',pin['url'],upstream],'clone')
            required(['git','-C',upstream,'checkout','--detach',pin['commit']],'checkout')
        if not (upstream/'.git').exists(): raise ValueError('independent upstream checkout required')
        if required(['git','-C',upstream,'rev-parse','HEAD'],'revision').strip()!=pin['commit']:
            raise ValueError('wrong source revision')
        if required(['git','-C',upstream,'status','--porcelain','--ignored','--untracked-files=all'],'clean').strip():
            raise ValueError('upstream checkout is not pristine')
        runtime = nera.parent/'libnera_runtime.a'
        report.update(upstream=pin,nera_sha256=sha(nera),runtime_sha256=sha(runtime),
            nera_source_commit=required(['git','rev-parse','HEAD'],'nera-head',ROOT).strip(),
            nera_worktree_status=required(['git','status','--porcelain'],'nera-status',ROOT),
            harness_sha256={p.name:sha(p) for p in HERE.iterdir() if p.is_file()},etf_decoder_sha256=sha(ETF_SOURCE),
            source_sha256={str(p.relative_to(upstream)):sha(p) for folder in ('src','include') for p in (upstream/folder).rglob('*') if p.is_file()})
        report['otp_version']=required(['erl','-noshell','-eval','io:format("~s",[erlang:system_info(system_version)]),halt().'],'otp').strip()
        # Follow the upstream build instructions in an external disposable copy.
        beam_source=work/'beam-source'
        if beam_source.exists(): shutil.rmtree(beam_source)
        shutil.copytree(upstream,beam_source,ignore=shutil.ignore_patterns('.git'))
        make = shutil.which('gmake') or shutil.which('make')
        if make is None:
            raise ValueError('GNU Make is required for the upstream BEAM build')
        required([make,'-j2','all'],'beam-build',beam_source,300)
        verify_beam_modules(beam_source/'ebin', MODULES + HTTP_DEPENDENCIES)
        for module in a.module or DEFAULT_MODULES:
            selected=[c for c in cases if c['module']==module]
            folder=work/module
            folder.mkdir(exist_ok=True)
            driver=folder/'cowlib_probe.erl'
            driver.write_text(source(selected))
            required(['erlc','-o',folder,driver],module+'-beam-driver')
            sources=[upstream/'src'/(module+'.erl')]
            if module=='cow_cookie': sources.append(upstream/'src/cow_date.erl')
            if module in ('cow_http', 'cow_http_hd'):
                closure = ('cow_http', *HTTP_DEPENDENCIES)
                sources.extend(upstream/'src'/(name+'.erl') for name in closure if name != module)
            exe=folder/'native'
            exe.unlink(missing_ok=True)
            _,build_rc=run([nera,'build',*sources,driver,'-I',upstream/'include','-e','cowlib_probe:start','-o',exe],module+'-nera-build',folder,300)
            m={'build_returncode':build_rc,'cases':[],
               'native_sources':[str(path.relative_to(upstream)) for path in sources]}
            report['modules'][module]=m
            if build_rc==0: m['executable_sha256']=sha(exe)
            for i,c in enumerate(selected):
                (folder/'case.txt').write_text(str(i))
                entry={k:c[k] for k in ('id','group','expect')}
                comparison=c.get('comparison', 'exact')
                entry['comparison']=comparison
                outcomes={}
                for engine,cmd in [('beam',['erl','-noshell','-pa',beam_source/'ebin',folder,'-eval','cowlib_probe:start(),halt().']),('nera',[exe])]:
                    if engine=='nera' and build_rc!=0:
                        entry['nera_error']='compilation blocked'; continue
                    out,rc=run(cmd,c['id']+'-'+engine,folder,a.timeout)
                    try:
                        outcomes[engine]=parse_output(out,c['id'],rc)
                        entry[engine]=outcomes[engine]
                        compare_outcome(outcomes[engine],outcomes[engine],c['expect'],comparison)
                    except (ValueError,TypeError) as e: entry[engine+'_error']=str(e)
                try:
                    if 'beam_error' in entry or 'nera_error' in entry or len(outcomes)!=2:
                        raise ValueError('missing or invalid engine result')
                    entry['wire_equal']=outcomes['beam']==outcomes['nera']
                    compare_outcome(outcomes['beam'],outcomes['nera'],c['expect'],comparison)
                    entry['status']='passed'
                except ValueError as e:
                    entry.update(status=('blocked' if build_rc != 0 and 'beam_error' not in entry else 'failed'),error=str(e))
                m['cases'].append(entry)
            m['passed']=sum(c['status']=='passed' for c in m['cases'])
            m['total']=len(selected)
            m['beam_passed']=sum('beam' in c and 'beam_error' not in c for c in m['cases'])
            m['blocked']=sum(c['status']=='blocked' for c in m['cases'])
            print(f'{module}: {m["passed"]}/{m["total"]}, native build {build_rc}',flush=True)
        if sha(nera)!=report['nera_sha256'] or sha(runtime)!=report['runtime_sha256']:
            raise ValueError('compiler/runtime changed during validation')
        report['status']='passed' if all(m['passed']==m['total'] for m in report['modules'].values()) else 'failed'
    except (OSError,ValueError) as e:
        report['error']=str(e)
        print(str(e),file=sys.stderr)
    finally:
        (work/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(f'Report: {work / "report.json"}')
    return 0 if report['status']=='passed' else 1

if __name__=='__main__': sys.exit(main())
