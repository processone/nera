#!/usr/bin/env python3
"""Replay original cow_date PropEr instances on BEAM and cumulative Nera.

Original sources, adapted copies, generated inputs and executables stay external.
This is finite predicate replay, not native PropEr generation or shrinking.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import subprocess
from runner import HERE, ROOT, external_workdir, sha, etf, parse_output
from upstream_cookie_date import PROPER_COMMIT
from upstream_http import adapt_source

GENERATORS = ('fixdate_gen', 'rfc850_gen', 'asctime_gen')
SEED = 19001


def adapt_date(original, false_control=False):
    if re.findall(r'^(prop_\w+)\(\) ->', original, re.M) != ['prop_http_date']:
        raise ValueError('unexpected upstream property inventory')
    span = re.search(r'^prop_http_date\(\) ->.*?(?=^\w+\()', original, re.M | re.S)
    if not span or original.count('?FORALL(') != 1:
        raise ValueError('unexpected property structure')
    adapted, _ = adapt_source(original, 'cow_date', properties=True)
    if span.group() not in adapted:
        raise ValueError('property body changed')
    adapted = adapted.replace('-module(cow_date).', '-module(cow_date).\n-export([prop_http_date/0]).', 1)
    if false_control:
        header = '-define(FORALL(Variable, Generator, Property), fun(Variable) -> Property end).'
        if adapted.count(header) != 1:
            raise ValueError('missing property macro')
        adapted = adapted.replace(header, header.replace('-> Property end', '-> false end'))
    return adapted


def validate_output(code, output, inputs, strict=True):
    lines = output.splitlines()
    if code != 0 or len(lines) != 2*len(inputs)+1 or lines[-1] != f'DATE_DONE:{len(inputs)}':
        raise ValueError('failed process or incomplete/extra output')
    records = []
    for i, sample in enumerate(inputs):
        result = parse_output('\n'.join(lines[2*i:2*i+2]), f'date_{i}', code)
        label, pair = sample[1]
        date, _ = pair[1]
        expected = ('tuple', (label, ('atom','true'), date, date))
        passed = result == expected
        if strict and not passed:
            raise ValueError(f'property or exact date mismatch for input {i}')
        records.append({'index':i, 'passed':passed, 'actual':result})
    return records


def driver(beam):
    check = '''P = case Control of
        true -> {forall,T,_} = cow_date:prop_http_date(),
                proper:check(proper:forall(T, fun(_) -> false end), [X], [quiet]);
        false -> proper:check(cow_date:prop_http_date(), [X], [quiet])
    end''' if beam else 'F = cow_date:prop_http_date(), P = F(X)'
    return '''-module(date_properties).
-export([start/0, false_control/0]).
start() ->
    {ok,B} = file:read_file("inputs.etf"),
    loop(binary_to_term(B), 0, false).
false_control() ->
    {ok,B} = file:read_file("inputs.etf"),
    loop([hd(binary_to_term(B))], 0, true).
loop([], I, _Control) -> io:format("DATE_DONE:~w~n", [I]);
loop([{Format,X}|Rest], I, Control) ->
    {Date,Bin} = X,
    '''+check+''',
    Result = {Format,P,Date,cow_date:parse_date(Bin)},
    io:format("COWLIB_CASE:date_~w:~w~n", [I,binary_to_list(term_to_binary(Result))]),
    io:format("COWLIB_DONE:date_~w~n", [I]),
    loop(Rest, I+1, Control).
'''


def coverage(inputs):
    result = {}
    for sample in inputs:
        label, pair = sample[1]
        date, _ = pair[1]
        day, clock = date[1]
        values = [v[1] for v in day[1]+clock[1]]
        group = result.setdefault(label[1], {'count':0, **{key:set() for key in ('years','months','days','hours','minutes','seconds')}})
        group['count'] += 1
        for key, value in zip(('years','months','days','hours','minutes','seconds'), values):
            group[key].add(value)
    return {label:{key:sorted(value) if isinstance(value,set) else value for key,value in group.items()} for label,group in result.items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('upstream','proper','nera','work-dir'):
        parser.add_argument('--'+name, type=Path, required=True)
    parser.add_argument('--samples-per-format', type=int, default=1000)
    args = parser.parse_args()
    if not 1 <= args.samples_per_format <= 1000:
        parser.error('samples-per-format must be in 1..1000')
    upstream, proper, nera = (p.resolve() for p in (args.upstream,args.proper,args.nera))
    work = external_workdir(args.work_dir)
    for source in (upstream,proper,ROOT):
        if work == source or work in source.parents or source in work.parents:
            parser.error('work directory must be separate from sources')
    work.mkdir(parents=True,exist_ok=True)
    env={k:v for k,v in os.environ.items() if k not in ('ERL_FLAGS','ERL_AFLAGS','ERL_ZFLAGS','ERL_LIBS','EUNIT')}
    env['ERL_LIBS']=str(proper/'_build/default/lib')
    report={'status':'failed','commands':[],'scope':__doc__,'platform':platform.platform(),
            'samples_per_format':args.samples_per_format,'seed':f'{{{SEED}, sample_index, format_index}}',
            'sizes':'1 + ((sample_index - 1) rem 50)'}
    def run(argv,label,cwd=work,timeout=300):
        argv=list(map(str,argv))
        try:
            proc=subprocess.run(argv,cwd=cwd,env=env,capture_output=True,timeout=timeout)
            out,err,code=proc.stdout,proc.stderr,proc.returncode
        except subprocess.TimeoutExpired as error:
            out,err,code=error.stdout or b'',error.stderr or b'','timeout'
        stdout,stderr=work/(label+'.stdout'),work/(label+'.stderr')
        stdout.write_bytes(out);stderr.write_bytes(err)
        report['commands'].append({'argv':argv,'label':label,'cwd':str(cwd),'returncode':code,
                                  'stdout_sha256':sha(stdout),'stderr_sha256':sha(stderr)})
        return out.decode(errors='replace'),code
    def require(argv,label,cwd=work):
        out,code=run(argv,label,cwd)
        if code != 0:raise ValueError(f'{label}: {code}; see external logs')
        return out.strip()
    try:
        pin=json.loads((HERE/'upstream.json').read_text())
        for label,path,commit in (('cowlib',upstream,pin['commit']),('proper',proper,PROPER_COMMIT)):
            external_workdir(path)
            if require(['git','-C',path,'rev-parse','HEAD'],label+'-head') != commit:
                raise ValueError('wrong '+label+' revision')
        if require(['git','-C',upstream,'status','--porcelain','--untracked-files=all','--ignored'],'cowlib-clean'):
            raise ValueError('dirty Cowlib source')
        if require(['git','-C',proper,'diff','HEAD','--','src','include','rebar.config'],'proper-clean'):
            raise ValueError('modified PropEr source')
        original=upstream/'src/cow_date.erl'
        report.update(upstream=pin,proper_commit=PROPER_COMMIT,original_sha256=sha(original),
                      nera_commit=require(['git','rev-parse','HEAD'],'nera-head',ROOT),
                      nera_status=require(['git','status','--porcelain'],'nera-status',ROOT),
                      nera_sha256=sha(nera),runtime_sha256=sha(nera.parent/'libnera_runtime.a'),
                      harness_sha256=sha(Path(__file__)),
                      proper_beam_sha256={p.name:sha(p) for p in sorted((proper/'_build/default/lib/proper/ebin').glob('*.beam'))},
                      otp=require(['erl','-noshell','-eval','io:format("~s",[erlang:system_info(system_version)]),halt().'],'otp'))
        if not report['proper_beam_sha256']:raise ValueError('build pinned PropEr first')
        beam=work/'beam';beam.mkdir(exist_ok=True)
        beam_driver=beam/'date_properties.erl';beam_driver.write_text(driver(True))
        require(['erlc','-DTEST','+export_all','+{parse_transform,eunit_autoexport}','-I',upstream/'include','-o',beam,original,beam_driver],'beam-build')
        beam_args=['erl','-noshell','-pa',beam,'-eval']
        expr='''
        Gs = [GENERATORS],
        Inputs = lists:append([[begin
            {ok,X} = proper_gen:pick(apply(cow_date,G,[]),1+((I-1) rem 50),{BASE_SEED,I,J}),
            {Name,X}
        end || I <- lists:seq(1,SAMPLES)] || {{Name,G},J} <- lists:zip(Gs,lists:seq(1,3))]),
        ok = file:write_file("inputs.etf",term_to_binary(Inputs)), halt().
        '''.replace('SAMPLES',str(args.samples_per_format)).replace('BASE_SEED',str(SEED)).replace(
            'GENERATORS', ','.join('{'+name.removesuffix('_gen')+','+name+'}' for name in GENERATORS))
        require([*beam_args,expr],'generate-first')
        inputs_path=work/'inputs.etf'; first=sha(inputs_path)
        require([*beam_args,expr],'generate-repeat')
        report['generation_repeated_identically']=sha(inputs_path)==first
        if not report['generation_repeated_identically']:raise ValueError('generation is not reproducible')
        inputs=etf(inputs_path.read_bytes())[1]
        expected=3*args.samples_per_format
        if len(inputs)!=expected:raise ValueError('incomplete input corpus')
        report.update(expected=expected,inputs_sha256=first,coverage=coverage(inputs))
        out,rc=run([*beam_args,'date_properties:start(),halt().'],'beam-replay')
        beam_records=validate_output(rc,out,inputs,strict=False)
        report['beam']={'passed':sum(x['passed'] for x in beam_records),'failed':[x for x in beam_records if not x['passed']]}
        native=work/'native';native.mkdir(exist_ok=True)
        src=native/'cow_date.erl';src.write_text(adapt_date(original.read_text()))
        native_driver=native/'date_properties.erl';native_driver.write_text(driver(False))
        report.update(adapted_sha256=sha(src),beam_driver_sha256=sha(beam_driver),native_driver_sha256=sha(native_driver))
        exe=native/'run';exe.unlink(missing_ok=True)
        require([nera,'build',src,native_driver,'--no-warnings','-I',upstream/'include','-e','date_properties:start','-o',exe],'native-build',native)
        out,rc=run([exe],'native-cumulative-replay')
        records=validate_output(rc,out,inputs,strict=False)
        report['native']={'passed':sum(x['passed'] for x in records),'failed':[x for x in records if not x['passed']],
                          'executable_sha256':sha(exe),'exact_results_equal':records==beam_records}
        out,rc=run([*beam_args,'date_properties:false_control(),halt().'],'beam-false-control')
        def rejected(out,rc):
            values=validate_output(rc,out,inputs[:1],strict=False)
            if len(values)!=1 or values[0]['actual'][1][1]!=('atom','false'):return False
            try:validate_output(rc,out,inputs[:1])
            except ValueError:return True
            return False
        report['beam_false_control_rejected']=rejected(out,rc)
        control=native/'false';control.mkdir(exist_ok=True)
        false_src=control/'cow_date.erl';false_src.write_text(adapt_date(original.read_text(),True))
        false_exe=control/'run';false_exe.unlink(missing_ok=True)
        require([nera,'build',false_src,native_driver,'--no-warnings','-I',upstream/'include','-e','date_properties:false_control','-o',false_exe],'native-false-build',control)
        out,rc=run([false_exe],'native-false-control')
        report['native_false_control_rejected']=rejected(out,rc)
        report['native_false_source_sha256']=sha(false_src)
        if sha(nera)!=report['nera_sha256'] or sha(nera.parent/'libnera_runtime.a')!=report['runtime_sha256']:
            raise ValueError('compiler/runtime changed during validation')
        if report['beam']['passed']==expected and report['native']['passed']==expected and report['native']['exact_results_equal'] and report['beam_false_control_rejected'] and report['native_false_control_rejected']:
            report['status']='passed'
    except (ValueError,OSError) as error:
        report['error']=str(error)
    finally:
        (work/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps({k:v for k,v in report.items() if k in ('status','error','expected','beam','native','beam_false_control_rejected','native_false_control_rejected')})[:4000],flush=True)
        print(work/'report.json',flush=True)
    return 0 if report['status']=='passed' else 1


if __name__=='__main__':
    raise SystemExit(main())
