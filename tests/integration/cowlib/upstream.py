#!/usr/bin/env python3
"""Execute pinned cow_qs EUnit on BEAM and original test closures in Nera."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
from runner import external_workdir, sha, HERE, ROOT

GENERATORS = {'parse_qs_test_':24, 'parse_qs_identity_test_':5,
              'qs_test_':16, 'qs_identity_test_':5, 'urldecode_test_':7,
              'urldecode_identity_test_':5, 'urlencode_test_':5,
              'urlencode_identity_test_':5}
COUNT = sum(GENERATORS.values())


def native_passed(code, output, index):
    return code == 0 and output == f'COW_QS_TEST:{index}\n'


def adapt_source(original):
    names = re.findall(r'^(\w+_test_)\(\) ->',original,re.M)
    if names != list(GENERATORS) or re.search(r'^\w+_test\(\)',original,re.M):
        raise ValueError('upstream functional test inventory changed')
    # At this pinned revision, each TEST section ends with only Horse benchmarks.
    removed = []
    def strip(match):
        block=match.group(0)
        if re.search(r'^\w+_test_?\(',block,re.M):
            raise ValueError('refusing to remove a functional test')
        removed.extend(re.findall(r'^(horse_\w+)\(\) ->',block,re.M))
        return ''
    native,n=re.subn(r'^horse_\w+\(\) ->.*?(?=^-endif\.)',strip,original,flags=re.M|re.S)
    if n!=4 or len(removed)!=17: raise ValueError('benchmark inventory changed')
    calls=', '.join(name+'()' for name in GENERATORS)
    inventory='\n'.join(f'    {count} = length({name}()),' for name,count in GENERATORS.items())
    adapter=f'''
nera_upstream_start() ->
    {{ok, B}} = file:read_file("index.txt"),
    I = binary_to_integer(B),
{inventory}
    Tests = lists:append([{calls}]),
    {COUNT} = length(Tests),
    {{_, F}} = lists:nth(I, Tests),
    F(),
    io:format("COW_QS_TEST:~w~n", [I]).
'''
    native=native.replace('-module(cow_qs).','-module(cow_qs).\n-define(TEST, true).\n-export([nera_upstream_start/0]).',1)
    return native+adapter,removed


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--upstream',required=True,type=Path)
    p.add_argument('--nera',required=True,type=Path)
    p.add_argument('--work-dir',required=True,type=Path)
    a=p.parse_args()
    work=external_workdir(a.work_dir); upstream=a.upstream.resolve(); nera=a.nera.resolve()
    if work==upstream or upstream in work.parents or work in upstream.parents:
        p.error('work directory must be separate from upstream')
    work.mkdir(parents=True,exist_ok=True)
    env={k:v for k,v in os.environ.items() if k not in ('ERL_FLAGS','ERL_AFLAGS','ERL_ZFLAGS','ERL_LIBS')}
    r={'status':'failed','commands':[],'generators':GENERATORS,'expected':COUNT,'native':[]}
    def run(cmd,label,cwd=work,timeout=180):
        cmd=list(map(str,cmd))
        try:
            out=subprocess.run(cmd,cwd=cwd,env=env,capture_output=True,timeout=timeout)
            stdout,stderr,code=out.stdout,out.stderr,out.returncode
        except subprocess.TimeoutExpired as e:
            stdout,stderr,code=e.stdout or b'',e.stderr or b'','timeout'
        (work/(label+'.stdout')).write_bytes(stdout);(work/(label+'.stderr')).write_bytes(stderr)
        r['commands'].append({'argv':cmd,'label':label,'returncode':code})
        return stdout.decode(errors='replace'),code
    def require(cmd,label,cwd=work):
        out,code=run(cmd,label,cwd)
        if code!=0: raise ValueError(f'{label}: {code}')
        return out
    try:
        pin=json.loads((HERE/'upstream.json').read_text())
        if not (upstream/'.git').exists(): raise ValueError('independent upstream checkout required')
        if require(['git','-C',upstream,'rev-parse','HEAD'],'revision').strip()!=pin['commit']: raise ValueError('wrong revision')
        if require(['git','-C',upstream,'status','--porcelain','--ignored','--untracked-files=all'],'clean').strip(): raise ValueError('dirty upstream')
        original=(upstream/'src/cow_qs.erl').read_text()
        adapted,removed=adapt_source(original)
        r.update(upstream=pin,source_sha256=sha(upstream/'src/cow_qs.erl'),nera_sha256=sha(nera),runtime_sha256=sha(nera.parent/'libnera_runtime.a'),
                 harness_sha256=sha(Path(__file__)),include_sha256={p.name:sha(p) for p in (upstream/'include').glob('*.hrl')},removed_benchmarks=removed,checkout_commit=require(['git','rev-parse','HEAD'],'head',ROOT).strip())
        r['otp_version']=require(['erl','-noshell','-eval','io:format("~s",[erlang:system_info(system_version)]),halt().'],'otp').strip()
        # Build using make, as prescribed upstream, with an additional focused target.
        build=work/'beam-source'
        if build.exists(): shutil.rmtree(build)
        shutil.copytree(upstream,build,ignore=shutil.ignore_patterns('.git'))
        (build/'qs-validation.mk').write_text('qs-validation:\n\tmkdir -p qs-ebin\n\terlc -DTEST +\'{parse_transform,eunit_autoexport}\' -I include -o qs-ebin src/cow_qs.erl\n')
        require(['make','-f','Makefile','-f','qs-validation.mk','qs-validation'],'beam-build',build)
        # Verify generator cardinalities in addition to EUnit's own total.
        check=','.join(f'{n}=length(cow_qs:{name}())' for name,n in GENERATORS.items())
        expr=check+',case eunit:test(cow_qs,[verbose]) of ok -> io:format("COW_QS_EUNIT_OK~n"),halt(0); _ -> halt(1) end.'
        out,code=run(['erl','-noshell','-pa',build/'qs-ebin','-eval',expr],'beam')
        r['beam_passed']=code==0 and f'All {COUNT} tests passed.' in out and 'COW_QS_EUNIT_OK' in out
        src=work/'cow_qs.erl';src.write_text(adapted)
        r['adapted_sha256']=sha(src)
        exe=work/'native';exe.unlink(missing_ok=True)
        _,code=run([nera,'build',src,'-I',upstream/'include','-e','cow_qs:nera_upstream_start','-o',exe],'native-build')
        r['native_build_returncode']=code
        if code==0:
            r['executable_sha256']=sha(exe)
            for i in range(1,COUNT+1):
                (work/'index.txt').write_text(str(i))
                out,code=run([exe],f'native-{i}',timeout=15)
                r['native'].append({'index':i,'returncode':code,'passed':native_passed(code,out,i)})
        if sha(nera)!=r['nera_sha256'] or sha(nera.parent/'libnera_runtime.a')!=r['runtime_sha256']: raise ValueError('compiler/runtime changed')
        r['native_passed']=sum(x['passed'] for x in r['native'])
        r['status']='passed' if r['beam_passed'] and r['native_passed']==COUNT else 'failed'
        r['scope']='Original EUnit on BEAM; original functional test closures via external adapter in Nera. Horse benchmarks excluded, no native EUnit framework claim.'
    except (OSError,ValueError) as e: r['error']=str(e)
    finally:
        (work/'report.json').write_text(json.dumps(r,indent=2)+'\n')
        print(f'{r["status"]}: BEAM {r.get("beam_passed")}, native {r.get("native_passed",0)}/{COUNT}; {work / "report.json"}')
    return 0 if r['status']=='passed' else 1

if __name__=='__main__': sys.exit(main())
