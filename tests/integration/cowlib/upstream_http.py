"""Replay the pinned HTTP upstream suites without vendoring source."""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
from runner import HERE, ROOT, HTTP_DEPENDENCIES, external_workdir, sha
from upstream_cookie_date import PROPER_COMMIT


def functional_inventory(source):
    return [(name, name.endswith('_')) for name in
            re.findall(r'^(\w+_test_?)\(\) ->', source, re.M)]


def passed(code, output, index):
    return code == 0 and output == f'COW_HTTP_UPSTREAM:{index}\n'



def property_closure(span):
    # This pinned suite has one outer FORALL per property. Preserve its predicate
    # text exactly, but drop the generation argument for externally supplied input.
    match = re.search(r'(prop_\w+\(\) ->\s*)\?FORALL\((.*)\)\.', span, re.S)
    if not match:
        raise ValueError('unsupported property envelope')
    args = match.group(2)
    depth = 0
    cuts = []
    # Strings, comments and character literals must not act as separators.
    tokens = re.finditer(r'"(?:\\.|[^"\\])*"|\$\\?.|%[^\n]*|<<|>>|[(){}\[\],]', args)
    for token in tokens:
        t = token.group()
        if t in ('(', '{', '[', '<<'):
            depth += 1
        elif t in (')', '}', ']', '>>'):
            depth -= 1
        elif t == ',' and depth == 0:
            cuts.append(token.start())
            if len(cuts) == 2:
                break
    if len(cuts) != 2:
        raise ValueError('unsupported FORALL arguments')
    variable = args[:cuts[0]].strip()
    predicate = args[cuts[1]+1:]
    closure = match.group(1) + 'fun(' + variable + ') ->' + predicate + ' end.'
    return span[:match.start()] + closure + span[match.end():]

def adapt_source(source, module, properties=False, property_envelope='macro', test_helpers=()):
    removed = []
    def section(match):
        block = match.group(1)
        if re.search(r"^-(?:ifdef|ifndef|else|endif)", block, re.M):
            raise ValueError('nested TEST conditionals unsupported')
        starts = list(re.finditer(r"^([a-z][A-Za-z0-9_]*)\(", block, re.M))
        result = block[:starts[0].start()] if starts else block
        for i, entry in enumerate(starts):
            name = entry.group(1)
            span = block[entry.start():starts[i + 1].start() if i + 1 < len(starts) else len(block)]
            keep = name in test_helpers or name.endswith(('_test', '_test_')) or (properties and
                       (name.startswith('prop_') or name in ('unquote', 'qvalue_to_iodata')))
            if keep:
                result += property_closure(span) if properties and name.startswith('prop_') and property_envelope == 'closure' else span
            elif name not in removed:
                removed.append(name)
        header = '-include_lib("proper/include/proper.hrl").'
        replacement = '-define(FORALL(Variable, Generator, Property), fun(Variable) -> Property end).' if properties and property_envelope == 'macro' else ''
        result = result.replace(header, replacement)
        return '-ifdef(TEST).\n' + result + '-endif.'
    result = re.sub(r'^-ifdef\(TEST\)\.\n(.*?)^-endif\.', section, source, flags=re.M | re.S)
    if result.count(f'-module({module}).') != 1:
        raise ValueError('unexpected module')
    result = result.replace(f'-module({module}).', f'-module({module}).\n-define(TEST, true).', 1)
    return result, removed


def entry_source(module, inventory, counts, properties):
    generators = [name+'()' for name, gen in inventory if gen]
    direct = [f'{{<<"{name}">>, fun {name}/0}}' for name, gen in inventory if not gen]
    checks = '\n'.join(f'    {counts[name]} = length({name}()),' for name, gen in inventory if gen)
    total = sum(counts.values())
    # All test bodies remain verbatim. EUnit treats normal completion as success.
    result = f'''
-export([nera_upstream_start/0]).
nera_upstream_start() ->
    {{ok, B}} = file:read_file("index.txt"),
    I = binary_to_integer(B),
{checks}
    Tests = lists:append([{', '.join(generators)}]) ++ [{', '.join(direct)}],
    {total} = length(Tests),
    {{_, F}} = lists:nth(I, Tests),
    F(),
    io:format("COW_HTTP_UPSTREAM:~w~n", [I]).
'''
    # Erlang export attributes must precede function declarations.
    result = result.replace('-export([nera_upstream_start/0]).\n', '')
    if properties:
        branches = ';\n'.join(f'        {name} -> {name}()' for name in properties)
        result += f'''
nera_upstream_property() ->
    {{ok, B}} = file:read_file("properties.etf"),
    {{ok, Index}} = file:read_file("index.txt"),
    I = binary_to_integer(Index),
    {{Name, X}} = lists:nth(I, binary_to_term(B)),
    F = case Name of
{branches}
    end,
    true = F(X),
    io:format("COW_HTTP_UPSTREAM:~w~n", [I]).
'''
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('upstream', 'proper', 'nera', 'work-dir'):
        parser.add_argument('--'+name, type=Path, required=True)
    parser.add_argument('--samples', type=int, default=100)
    parser.add_argument('--property-envelope', choices=('macro', 'closure'), default='macro',
                        help='macro exercises Nera preprocessing; closure reproduces the legacy envelope workaround')
    args = parser.parse_args()
    if not 1 <= args.samples <= 1000:
        parser.error('--samples must be in 1..1000')
    upstream, proper, nera = (p.resolve() for p in (args.upstream, args.proper, args.nera))
    work = external_workdir(args.work_dir)
    for path in (upstream, proper, ROOT):
        if work == path or work in path.parents or path in work.parents:
            parser.error('work directory must be separate from sources')
    work.mkdir(parents=True, exist_ok=True)
    env = {k:v for k,v in os.environ.items() if k not in ('ERL_FLAGS', 'ERL_AFLAGS', 'ERL_ZFLAGS', 'ERL_LIBS')}
    env['ERL_LIBS'] = str(proper / '_build/default/lib')
    report = {'status':'failed', 'platform':platform.platform(), 'commands':[], 'modules':{},
              'scope':'Original EUnit on BEAM; original test closures and property predicates via external adapters on Nera. Native framework probes recorded separately; no native PropEr generation/shrinking claim.'}

    def run(cmd, label, cwd=work, timeout=300):
        cmd = list(map(str, cmd))
        try:
            value = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, timeout=timeout)
            out, err, code = value.stdout, value.stderr, value.returncode
        except subprocess.TimeoutExpired as error:
            out, err, code = error.stdout or b'', error.stderr or b'', 'timeout'
        stdout, stderr = work/(label+'.stdout'), work/(label+'.stderr')
        stdout.write_bytes(out); stderr.write_bytes(err)
        report['commands'].append({'argv':cmd, 'label':label, 'cwd':str(cwd), 'returncode':code,
            'stdout_sha256':sha(stdout), 'stderr_sha256':sha(stderr)})
        return out.decode(errors='replace'), code

    def require(cmd, label, cwd=work):
        output, code = run(cmd, label, cwd)
        if code != 0:
            raise ValueError(f'{label}: {code}; see external logs')
        return output.strip()

    try:
        pin = json.loads((HERE/'upstream.json').read_text())
        for name, repo, commit in [('cowlib', upstream, pin['commit']), ('proper', proper, PROPER_COMMIT)]:
            external_workdir(repo)
            if not (repo/'.git').exists():
                raise ValueError('independent checkout required')
            if require(['git','-C',repo,'rev-parse','HEAD'],name+'-head') != commit:
                raise ValueError('wrong '+name+' revision')
            flags = ['--ignored'] if name == 'cowlib' else []
            if require(['git','-C',repo,'status','--porcelain','--untracked-files=all',*flags],name+'-clean'):
                raise ValueError('dirty '+name+' checkout')
        runtime = nera.parent/'libnera_runtime.a'
        report.update(upstream=pin, proper_commit=PROPER_COMMIT,
            nera_commit=require(['git','rev-parse','HEAD'],'nera-head',ROOT),
            nera_worktree_status=require(['git','status','--porcelain'],'nera-status',ROOT),
            nera_sha256=sha(nera), runtime_sha256=sha(runtime), harness_sha256=sha(Path(__file__)),
            source_sha256={p.name:sha(p) for p in (upstream/'src').glob('*.erl')},
            include_sha256={p.name:sha(p) for p in (upstream/'include').glob('*.hrl')},
            proper_beam_sha256={p.name:sha(p) for p in (proper/'_build/default/lib/proper/ebin').glob('*.beam')},
            otp_version=require(['erl','-noshell','-eval','io:format("~s",[erlang:system_info(system_version)]),halt().'],'otp'))
        if not report['proper_beam_sha256']:
            raise ValueError('build pinned PropEr first')
        build = work/'beam-source'
        if build.exists():
            shutil.rmtree(build)
        shutil.copytree(upstream, build, ignore=shutil.ignore_patterns('.git'))
        # Build production dependencies using the upstream make target, then
        # compile the two original TEST modules with standard EUnit autoexport.
        require([shutil.which('gmake') or 'make', 'all'], 'beam-dependencies', build)
        beam = work/'beam'; beam.mkdir(exist_ok=True)
        require(['erlc','-DTEST','+export_all','+{parse_transform,eunit_autoexport}',
                 '-I',upstream/'include','-o',beam,upstream/'src/cow_http.erl',
                 upstream/'src/cow_http_hd.erl'],'beam-build')
        beam_args = ['erl','-noshell','-pa',build/'ebin',beam,'-eval']
        for module in ('cow_http', 'cow_http_hd'):
            original = (upstream/'src'/f'{module}.erl').read_text()
            inventory = functional_inventory(original)
            props = re.findall(r'^(prop_\w+)\(\) ->', original, re.M)
            expression = ','.join(f'io:format("INVENTORY:{name}:~w~n",[{"length("+module+":"+name+"())" if gen else "1"}])' for name,gen in inventory)+',halt().'
            out = require([*beam_args, expression],module+'-inventory')
            counts = {name:int(count) for name,count in re.findall(r'^INVENTORY:(\w+):(\d+)$',out,re.M)}
            if list(counts) != [name for name,_ in inventory]:
                raise ValueError('incomplete inventory')
            count = sum(counts.values())
            record = {'inventory':counts, 'expected':count, 'properties':props, 'native':[]}
            report['modules'][module] = record
            expr = f'case eunit:test({module},[verbose]) of ok -> io:format("UPSTREAM_EUNIT_OK~n"),halt(0); _ -> halt(1) end.'
            out, code = run([*beam_args,expr],module+'-beam')
            record['beam'] = {'returncode':code, 'passed':code==0 and f'All {count} tests passed.' in out and 'UPSTREAM_EUNIT_OK' in out}
            directory = work/module; directory.mkdir(exist_ok=True)
            adapted, removed = adapt_source(original, module)
            adapted = adapted.replace(f'-module({module}).', f'-module({module}).\n-export([nera_upstream_start/0]).',1)
            adapted += entry_source(module,inventory,counts,[])
            src = directory/f'{module}.erl'; src.write_text(adapted)
            record.update(adapted_sha256=sha(src), removed_for_eunit_adapter=removed)
            deps = [upstream/'src'/f'{dep}.erl' for dep in HTTP_DEPENDENCIES if dep != module]
            if module == 'cow_http_hd':
                deps = [upstream/'src'/f'{dep}.erl' for dep in ('cow_http', *HTTP_DEPENDENCIES) if dep != module]
            exe = directory/'run'; exe.unlink(missing_ok=True)
            _, code = run([nera,'build',src,*deps,'-I',upstream/'include','-e',module+':nera_upstream_start','-o',exe],module+'-native-build')
            record['native_build_returncode'] = code
            if code == 0:
                record['executable_sha256'] = sha(exe)
                for i in range(1,count+1):
                    (directory/'index.txt').write_text(str(i))
                    out, rc = run([exe],f'{module}-native-{i}',directory,15)
                    record['native'].append({'index':i, 'returncode':rc, 'passed':passed(rc,out,i),
                        **({'stdout':out[:3000], 'stderr':(work/f'{module}-native-{i}.stderr').read_text(errors='replace')[:3000]} if not passed(rc,out,i) else {})})
            record['native_passed'] = sum(x['passed'] for x in record['native'])
            record['native_blocked'] = count if code != 0 else 0
            print(f'{module}: BEAM {record["beam"]["passed"]}, native {record["native_passed"]}/{count}, blocked {record["native_blocked"]}',flush=True)
            # Probe actual framework calls independently of the closure adapter.
            probe = directory/'framework_probe.erl'
            probe.write_text(f'-module(framework_probe).\n-export([start/0]).\nstart() -> eunit:test({module}, [verbose]).\n')
            _, rc = run([nera,'build',src,*deps,probe,'-I',upstream/'include','-e','framework_probe:start','-o',directory/'framework'],module+'-eunit-framework-build')
            record['native_eunit_framework_build_returncode'] = rc
            if rc == 0:
                out, rc = run([directory/'framework'],module+'-eunit-framework-run',directory)
                record['native_eunit_framework'] = {'returncode':rc,'output':out[:3000]}
            if props:
                run_properties(module,original,inventory,counts,props,directory,deps,beam_args,args.samples,nera,upstream,run,require,record,args.property_envelope)
        if sha(nera)!=report['nera_sha256'] or sha(runtime)!=report['runtime_sha256']:
            raise ValueError('compiler/runtime changed during validation')
        # Success is the explicit adapter scope, not native framework compatibility.
        report['status'] = 'passed' if all(r['beam']['passed'] and r['native_passed']==r['expected'] and
            (not r['properties'] or r['property_replay']['passed']) for r in report['modules'].values()) else 'failed'
    except (OSError,ValueError) as error:
        report['error'] = str(error)
    finally:
        (work/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(f'{report["status"]}: {work / "report.json"}',flush=True)
    return 0 if report['status']=='passed' else 1


def run_properties(module,original,inventory,counts,props,directory,deps,beam_args,samples,nera,upstream,run,require,record,property_envelope):
    replay = {'samples_per_property':samples, 'property_envelope':property_envelope, 'seed':'{17001, sample_index, property_index}',
              'sizes':'1 + ((sample_index - 1) rem 50); positive sizes allow non_empty generators', 'native':[], 'beam':[],
              'scope':'Original PropEr generators on BEAM with fixed seeds; same inputs replay original predicates on both engines, in isolated Nera processes.'}
    record['property_replay'] = replay
    prop_atoms = ','.join(props)
    expr = f'''
    Props = [{prop_atoms}],
    Inputs = lists:append([begin
        {{forall, Type, _}} = apply({module}, Name, []),
        [begin
            {{ok, X}} = proper_gen:pick(Type, 1 + ((I - 1) rem 50), {{17001,I,J}}),
            {{Name, X}}
        end || I <- lists:seq(1,{samples})]
    end || {{Name,J}} <- lists:zip(Props,lists:seq(1,length(Props)))]),
    ok = file:write_file("properties.etf", term_to_binary(Inputs)),
    [begin
        R = proper:check(apply({module},Name,[]), [X], [quiet]),
        io:format("PROPERTY:~w:~w~n",[I,R])
    end || {{{{Name,X}},I}} <- lists:zip(Inputs,lists:seq(1,length(Inputs)))],
    {{forall, T, _}} = apply({module}, hd(Props), []),
    {{_, First}} = hd(Inputs),
    false = proper:check(proper:forall(T, fun(_) -> false end), [First], [quiet]),
    io:format("FALSE_PROPERTY_REJECTED~n"), halt().
    '''
    out, code = run([*beam_args,expr],module+'-properties-generate-beam',directory,600)
    replay['beam_returncode'] = code
    replay['beam'] = [{'index':int(i),'passed':result=='true'} for i,result in re.findall(r'^PROPERTY:(\d+):(true|false)$',out,re.M)]
    replay['beam_false_control_rejected'] = code==0 and 'FALSE_PROPERTY_REJECTED' in out
    total = len(props)*samples
    replay['expected'] = total
    replay['beam_passed'] = sum(x['passed'] for x in replay['beam'])
    inputs = directory/'properties.etf'
    if code!=0 or not inputs.exists() or len(replay['beam'])!=total:
        replay.update(passed=False,error='BEAM generation/replay incomplete')
        return
    replay['inputs_sha256'] = sha(inputs)
    source,removed = adapt_source(original,module,properties=True,property_envelope=property_envelope)
    source = source.replace(f'-module({module}).',f'-module({module}).\n-export([nera_upstream_start/0, nera_upstream_property/0]).',1)
    source += entry_source(module,inventory,counts,props)
    prop_dir = directory/'property-source'; prop_dir.mkdir(exist_ok=True)
    src = prop_dir/f'{module}.erl'; src.write_text(source)
    replay.update(adapted_sha256=sha(src),removed=removed)
    exe = directory/'property-run'; exe.unlink(missing_ok=True)
    _,code = run([nera,'build',src,*deps,'-I',upstream/'include','-e',module+':nera_upstream_property','-o',exe],module+'-properties-native-build')
    replay['native_build_returncode'] = code
    if code==0:
        replay['executable_sha256'] = sha(exe)
        for i in range(1,total+1):
            (directory/'index.txt').write_text(str(i))
            out,rc = run([exe],f'{module}-property-{i}',directory,15)
            okay = passed(rc,out,i)
            replay['native'].append({'index':i,'property':props[(i-1)//samples], 'sample':(i-1)%samples+1,
                'returncode':rc,'passed':okay,
                **({'stdout':out[:3000],'stderr':(directory.parent/f'{module}-property-{i}.stderr').read_text(errors='replace')[:3000]} if not okay else {})})
        # The same common adapter must reject a false assertion on Nera too.
        changed = source.replace('true = F(X)', 'true = false', 1)
        control_dir = directory/'false-control';control_dir.mkdir(exist_ok=True)
        control = control_dir/f'{module}.erl';control.write_text(changed)
        false_exe = directory/'false-run';false_exe.unlink(missing_ok=True)
        _,rc=run([nera,'build',control,*deps,'-I',upstream/'include','-e',module+':nera_upstream_property','-o',false_exe],module+'-false-build')
        replay['native_false_build_returncode']=rc
        if rc==0:
            (directory/'index.txt').write_text('1')
            out,rc=run([false_exe],module+'-false-run',directory,15)
            replay['native_false_control_rejected']=isinstance(rc,int) and rc!=0 and 'badmatch' in out+(directory.parent/(module+'-false-run.stderr')).read_text(errors='replace')
    replay['native_passed']=sum(x['passed'] for x in replay['native'])
    replay['native_blocked']=total if code!=0 else 0
    replay['passed']=(replay['beam_passed']==total and replay['native_passed']==total and
                      replay['beam_false_control_rejected'] and replay.get('native_false_control_rejected',False))
    print(f'{module} properties: BEAM {replay["beam_passed"]}/{total}, native {replay["native_passed"]}/{total}',flush=True)


if __name__ == '__main__':
    sys.exit(main())
