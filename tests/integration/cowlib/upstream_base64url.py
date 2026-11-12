#!/usr/bin/env python3
"""Replay original Cowlib Base64url properties on BEAM/PropEr and native Nera.

Sources, adapted modules, generated inputs and executables stay outside the repo.
This checks finite property instances, not native PropEr generation/shrinking.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
from runner import HERE, ROOT, external_workdir, sha, parse_output
from upstream_cookie_date import PROPER_COMMIT

SEED = 'nera-cow-base64url-v1'
PROPERTIES = ('prop_identity', 'prop_identity_no_padding')


def corpus(seed=SEED):
    # SHAKE-256 gives portable, version-independent bytes; no host PRNG state.
    def sample(index, size):
        return hashlib.shake_256(f'{seed}:{index}'.encode()).digest(size)
    values = [sample(i, i) for i in range(257)]
    values += [bytes([i]) for i in range(256)]
    values += [bytes(range(256)), bytes(range(256)) * 64]
    values += [bytes([b]) * n for b in (0, 255, 85, 170)
               for n in (2, 3, 4, 255, 256, 257, 1023, 1024, 1025)]
    values += [sample(f'boundary-{n}', n) for n in
               (511, 512, 513, 1023, 1024, 1025, 4095, 4096, 4097, 16383, 16384, 16385)]
    values += [sample(f'random-{i}', int.from_bytes(sample(f'length-{i}', 2), 'big') % 4097)
               for i in range(1000)]
    return values


def pack_inputs(values):
    return b''.join(len(v).to_bytes(4, 'big') + v for v in values)


def unpack_inputs(raw):
    result = []
    while raw:
        if len(raw) < 4:
            raise ValueError('truncated length')
        size = int.from_bytes(raw[:4], 'big')
        if len(raw) < size + 4:
            raise ValueError('truncated input')
        result.append(raw[4:4+size])
        raw = raw[4+size:]
    return result


def validate_output(code, output, values, strict=True):
    lines = output.splitlines()
    if code != 0 or len(lines) != 2 * len(values) + 1 or lines[-1] != f'BASE64_DONE:{len(values)}':
        raise ValueError('failed process or incomplete/extra output')
    results = []
    for i, value in enumerate(values):
        result = parse_output('\n'.join(lines[2*i:2*i+2]), f'b64_{i}', code)
        padded = base64.urlsafe_b64encode(value)
        expected = ('tuple', (('atom', 'true'), ('atom', 'true'), ('binary', padded.hex()), ('binary', padded.rstrip(b'=').hex())))
        if strict and result != expected:
            raise ValueError(f'property or encoding mismatch for input {i}')
        results.append({'index':i, 'length':len(value), 'passed':result==expected, 'actual':result})
    return results


def summarize(records, values):
    def actual(record):
        return record.get('actual', ('tuple', ()))[1]
    return {
        'cases':len(records),
        'passed_per_property':{name:sum(len(actual(x))==4 and actual(x)[i]==('atom','true') for x in records) for i,name in enumerate(PROPERTIES)},
        'passed_encodings':sum(actual(x)[2:]==(('binary',base64.urlsafe_b64encode(v).hex()), ('binary',base64.urlsafe_b64encode(v).rstrip(b'=').hex())) for x,v in zip(records,values)),
        'rfc7515_test':'passed' if any('actual' in x for x in records) else 'unconfirmed',
        'failures':[x for x in records if not x['passed']],
    }


def force_failure(original, property_name):
    if property_name not in PROPERTIES:
        raise ValueError('unknown property')
    changed, count = re.subn(r'(' + property_name + r'\(\) ->\s*)\?FORALL', r'\1?FAIL_FORALL', original)
    if count != 1:
        raise ValueError('cannot inject property failure')
    return changed.replace('-ifdef(TEST).', '-ifdef(TEST).\n-define(FAIL_FORALL(V, G, P), ?FORALL(V, G, false)).', 1)


def adapt(original):
    # Explicit inventory, plus the pinned pristine checkout, guards this narrow adapter.
    tests = re.findall(r'^(\w+_test)\(\)', original, re.M)
    props = re.findall(r'^(prop_\w+)\(\)', original, re.M)
    if tests != ['rfc7515_test'] or props != list(PROPERTIES):
        raise ValueError('upstream property/test inventory changed')
    header = '-include_lib("proper/include/proper.hrl").'
    if original.count(header) != 1 or original.count('?FORALL(') != 2:
        raise ValueError('unsupported property structure')
    source = original.replace(header, '-define(FORALL(Variable, Generator, Property), fun(Variable) -> Property end).')
    exports = '-define(TEST, true).\n-export([rfc7515_test/0, prop_identity/0, prop_identity_no_padding/0]).'
    return source.replace('-module(cow_base64url).', '-module(cow_base64url).\n' + exports, 1)


def driver(beam):
    check = ('P = proper:check(cow_base64url:prop_identity(), [B], [quiet]),\n'
             '    Q = proper:check(cow_base64url:prop_identity_no_padding(), [B], [quiet])') if beam else (
             'F = cow_base64url:prop_identity(), P = F(B),\n'
             '    G = cow_base64url:prop_identity_no_padding(), Q = G(B)')
    return '''-module(base64_properties).
-export([start/0]).
start() ->
    ok = cow_base64url:rfc7515_test(),
    {ok, Data} = file:read_file("inputs.bin"),
    loop(Data, 0).
loop(<<>>, I) -> io:format("BASE64_DONE:~w~n", [I]);
loop(<<N:32, B:N/binary, Rest/binary>>, I) ->
    ''' + check + ''',
    Result = {P, Q, cow_base64url:encode(B), cow_base64url:encode(B, #{padding => false})},
    io:format("COWLIB_CASE:b64_~w:~w~n", [I, binary_to_list(term_to_binary(Result))]),
    io:format("COWLIB_DONE:b64_~w~n", [I]),
    loop(Rest, I + 1).
'''


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('upstream', 'proper', 'nera', 'work-dir'):
        p.add_argument('--'+name, required=True, type=Path)
    a = p.parse_args()
    upstream, proper, nera = a.upstream.resolve(), a.proper.resolve(), a.nera.resolve()
    work = external_workdir(a.work_dir)
    for other in (upstream, proper):
        external_workdir(other)
        if work == other or work in other.parents or other in work.parents:
            p.error('work directory must be separate from dependencies')
    work.mkdir(parents=True, exist_ok=True)
    env = {k:v for k,v in os.environ.items() if k not in ('ERL_FLAGS','ERL_AFLAGS','ERL_ZFLAGS','ERL_LIBS')}
    env['ERL_LIBS'] = str(proper / '_build/default/lib')
    report = {'status':'failed', 'commands':[], 'platform':platform.platform(), 'properties':list(PROPERTIES),
              'scope':'Deterministic instances of original properties: real PropEr replay on BEAM, external closure adapter on Nera; no native PropEr claim.'}

    def run(cmd, label, cwd=work, timeout=300):
        cmd = list(map(str, cmd))
        try:
            r = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, timeout=timeout)
            out, err, rc = r.stdout, r.stderr, r.returncode
        except subprocess.TimeoutExpired as e:
            out, err, rc = e.stdout or b'', e.stderr or b'', 'timeout'
        (work/(label+'.stdout')).write_bytes(out)
        (work/(label+'.stderr')).write_bytes(err)
        report['commands'].append({'argv':cmd, 'label':label, 'returncode':rc})
        return rc, out.decode(errors='replace')

    def require(cmd, label, cwd=work):
        rc, out = run(cmd, label, cwd)
        if rc != 0:
            raise ValueError(f'{label}: {rc}; see external logs')
        return out.strip()

    try:
        pin = json.loads((HERE/'upstream.json').read_text())
        for name, repo, commit in (('cowlib', upstream, pin['commit']), ('proper', proper, PROPER_COMMIT)):
            if not (repo/'.git').exists():
                raise ValueError('independent dependency checkout required')
            if require(['git','-C',repo,'rev-parse','HEAD'],name+'-revision') != commit:
                raise ValueError('wrong '+name+' revision')
            flags = ['--ignored'] if name == 'cowlib' else []  # PropEr has ignored build artifacts.
            if require(['git','-C',repo,'status','--porcelain','--untracked-files=all',*flags],name+'-clean'):
                raise ValueError('dirty '+name+' checkout')
        src = upstream/'src/cow_base64url.erl'
        original = src.read_text()
        adapted = adapt(original)
        runtime = nera.parent/'libnera_runtime.a'
        report.update(upstream=pin, proper_commit=PROPER_COMMIT, source_sha256=sha(src),
                      nera_sha256=sha(nera), runtime_sha256=sha(runtime), harness_sha256=sha(Path(__file__)),
                      nera_source_commit=require(['git','rev-parse','HEAD'],'head',ROOT),
                      worktree_status=require(['git','status','--porcelain'],'worktree',ROOT),
                      otp_version=require(['erl','-noshell','-eval','io:format("~s",[erlang:system_info(system_version)]),halt().'],'otp'))
        values = corpus()
        inputs = pack_inputs(values)
        (work/'inputs.bin').write_bytes(inputs)
        report['corpus'] = {'seed':SEED, 'algorithm':'SHAKE-256 plus explicit byte/boundary patterns; framed with uint32 big-endian lengths',
                            'count':len(values), 'sha256':sha(work/'inputs.bin'), 'lengths':[len(v) for v in values]}
        beam = work/'beam'; beam.mkdir(exist_ok=True)
        native = work/'native'; native.mkdir(exist_ok=True)
        (beam/'base64_properties.erl').write_text(driver(True))
        (native/'cow_base64url.erl').write_text(adapted)
        (native/'base64_properties.erl').write_text(driver(False))
        report['adapted_sha256'] = sha(native/'cow_base64url.erl')
        report['proper_beam_sha256'] = {f.name:sha(f) for f in (proper/'_build/default/lib/proper/ebin').glob('*.beam')}
        if not report['proper_beam_sha256']:
            raise ValueError('build pinned PropEr with rebar3 compile first')
        require(['erlc','-DTEST','+export_all','-o',beam,src,beam/'base64_properties.erl'],'beam-build')
        exe = native/'run'; exe.unlink(missing_ok=True)
        require([nera,'build',native/'cow_base64url.erl',native/'base64_properties.erl','-e','base64_properties:start','-o',exe],'native-build')
        report['executable_sha256'] = sha(exe)
        outcomes = {}
        for engine, cmd in (('beam',['erl','-noshell','-pa',beam,'-eval','base64_properties:start(),halt().']), ('nera',[exe])):
            rc, out = run(cmd,engine)
            try:
                outcomes[engine] = validate_output(rc,out,values,strict=False)
                report[engine] = summarize(outcomes[engine], values)
            except ValueError as error:
                outcomes[engine] = []
                report[engine] = {'error':str(error), 'last_completion':next((line for line in reversed(out.splitlines()) if line.startswith('COWLIB_DONE:')),None)}
            report[engine].update(returncode=rc, output_sha256=sha(work/(engine+'.stdout')), stderr_sha256=sha(work/(engine+'.stderr')))
        # Independent processes distinguish per-input behavior from cumulative
        # execution. A failed cumulative run still fails the overall verdict.
        isolated = []
        for index,value in enumerate(values):
            (work/'inputs.bin').write_bytes(pack_inputs([value]))
            rc,out = run([exe],f'isolated-{index}',timeout=30)
            try:
                record = validate_output(rc,out,[value],strict=False)[0]
                record['index'] = index
            except ValueError as error:
                record = {'index':index,'length':len(value),'passed':False,'error':str(error),'returncode':rc}
            isolated.append(record)
        report['nera_isolated'] = summarize(isolated, values)
        # Each property is forced false independently, in external copies only.
        # BEAM still uses the real PropEr replay path for these controls.
        (work/'inputs.bin').write_bytes(pack_inputs([b'']))
        report['negative_controls'] = {}
        for property_name in PROPERTIES:
            negative = work/('negative-' + property_name)
            negative.mkdir(exist_ok=True)
            beam_negative = negative/'beam'; beam_negative.mkdir(exist_ok=True)
            native_negative = negative/'native'; native_negative.mkdir(exist_ok=True)
            (beam_negative/'cow_base64url.erl').write_text(force_failure(original,property_name))
            (beam_negative/'base64_properties.erl').write_text(driver(True))
            (native_negative/'cow_base64url.erl').write_text(force_failure(adapted,property_name))
            (native_negative/'base64_properties.erl').write_text(driver(False))
            require(['erlc','-DTEST','+export_all','-o',beam_negative,beam_negative/'cow_base64url.erl',beam_negative/'base64_properties.erl'],property_name+'-negative-beam-build')
            badexe = native_negative/'run'; badexe.unlink(missing_ok=True)
            require([nera,'build',native_negative/'cow_base64url.erl',native_negative/'base64_properties.erl','-e','base64_properties:start','-o',badexe],property_name+'-negative-native-build')
            for engine,cmd in (('beam',['erl','-noshell','-pa',beam_negative,'-eval','base64_properties:start(),halt().']),('nera',[badexe])):
                label = property_name+'-negative-'+engine
                rc,out = run(cmd,label)
                err = (work/(label+'.stderr')).read_text(errors='replace')
                try:
                    validate_output(rc,out,[b''])
                    caught = False
                except ValueError:
                    # Must be the deliberately false property, not a crash.
                    records = validate_output(rc,out,[b''],strict=False)
                    caught = records[0]['actual'][1][PROPERTIES.index(property_name)] == ('atom','false')
                report['negative_controls'][label] = {'returncode':rc,'false_property_rejected':caught}
                if not caught:
                    raise ValueError('negative control failed for '+label)
        if sha(nera)!=report['nera_sha256'] or sha(runtime)!=report['runtime_sha256']:
            raise ValueError('compiler/runtime changed')
        report['status'] = 'passed' if all(len(records)==len(values) and all(x['passed'] for x in records) for records in outcomes.values()) and all(x['passed'] for x in isolated) else 'failed'
    except (OSError,ValueError) as e:
        report['error'] = str(e)
    finally:
        if 'inputs' in locals():
            (work/'inputs.bin').write_bytes(inputs)
        (work/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(f'{report["status"]}: {report.get("error", "see per-engine and cumulative results")}; {work / "report.json"}')
    return 0 if report['status']=='passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
