#!/usr/bin/env python3
"""Run original cow_base64url properties through native PropEr and BEAM."""
import base64
import hashlib
import json
from pathlib import Path
import re
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent/'proper'))
from generation import SEEDS, plain, _etf, _ETF_SOURCE
import native as proper_native

PROPERTIES = ('prop_identity', 'prop_identity_no_padding')


def configure(parser):
    parser.add_argument('--upstream', type=Path, required=True, help='pristine pinned external Cowlib checkout')
    parser.add_argument('--samples', type=int, default=1000, help='trials per property/seed/repetition (4..1000)')


def validate_args(args, parser):
    if not 4 <= args.samples <= 1000:
        parser.error('--samples must be in 4..1000')
    upstream, work = args.upstream.resolve(), args.work_dir.resolve()
    repo = HERE.parents[2]
    if (upstream == repo or repo in upstream.parents or upstream in repo.parents
            or work == upstream or upstream in work.parents or work in upstream.parents):
        parser.error('Cowlib sources and work directory must be separate and external')


def driver(args):
    return r'''-module(proper_probe).
-export([start/0]).
start() ->
    ok = cow_base64url:rfc7515_test(),
    lists:foreach(fun(Name) ->
        lists:foreach(fun(Seed) ->
            Result = run(Name, Seed),
            Result = run(Name, Seed),
            io:format("COW_BASE64URL_PROPER:~w~n", [binary_to_list(term_to_binary({Name,Seed,Result}))])
        end, [{101,202,303},{7,11,13},{999,1001,2027}])
    end, [prop_identity, prop_identity_no_padding]),
    io:format("COW_BASE64URL_PROPER_DONE~n").
seeded(Seed, Property) ->
    proper:setup(fun() ->
        rand:seed(exsplus,Seed),
        put(base64url_trace,[]),
        fun() -> ok end
    end,Property).
record(Value) -> put(base64url_trace,[Value|get(base64url_trace)]), ok.
trace() -> lists:reverse(erase(base64url_trace)).
run(Name,Seed) ->
    %% Preserve the generator and predicate returned by the actual upstream property.
    Original = apply(cow_base64url,Name,[]),
    {forall,Type,Predicate} = Original,
    Observed = proper:forall(Type,fun(B) ->
        record({B,cow_base64url:encode(B),cow_base64url:encode(B,#{padding=>false})}),
        Predicate(B)
    end),
    true = proper:quickcheck(seeded(Seed,Observed),
        [quiet,{numtests,SAMPLE_COUNT},{start_size,0},{max_size,256}]),
    Samples = trace(),
    SAMPLE_COUNT = length(Samples),
    False = proper:forall(Type,fun(B) -> record(B), Predicate(B) andalso false end),
    false = proper:quickcheck(seeded(Seed,False),
        [quiet,{numtests,100},{start_size,20},{max_size,40},{max_shrinks,1000}]),
    [<<>>] = proper:counterexample(),
    Shrinking = trace(),
    true = hd(Shrinking) =/= <<>>,
    false = proper:check(seeded(Seed,False),[<<>>],[quiet]),
    [<<>>] = trace(),
    true = proper:check(seeded(Seed,Original),[<<>>],[quiet]),
    [] = trace(),
    {Samples,Shrinking,[<<>>]}.
'''.replace('SAMPLE_COUNT', str(args.samples))


def prepare(args, work, report, run, beam):
    return prepare_module(args,work,report,run,beam,'cow_base64url',PROPERTIES,
                          ('prop_identity/0','prop_identity_no_padding/0','rfc7515_test/0'))


def prepare_module(args, work, report, run, beam, module_name, properties, exports):
    upstream = args.upstream.resolve()
    pin = json.loads((HERE/'upstream.json').read_text())
    head = run('cowlib-revision',['git','rev-parse','HEAD'],upstream)
    clean = run('cowlib-status',['git','status','--porcelain','--untracked-files=all'],upstream)
    if head['returncode'] != 0 or head['stdout'].strip() != pin['commit'] or clean['returncode'] != 0 or clean['stdout'].strip():
        raise ValueError('requires pinned pristine Cowlib checkout')
    original = upstream/'src'/(module_name+'.erl')
    source = original.read_text()
    if tuple(re.findall(r'^(prop_\w+)\(\) ->',source,re.M)) != properties:
        raise ValueError('unexpected upstream property inventory')
    # Ordinary OTP preprocessing/parse transforms, outside both repositories.
    # No property-body adapters or FORALL replacement.
    dependency = work/'prepared-cowlib-property'
    generated = dependency/'src'
    generated.mkdir(parents=True)
    proper_ebin = args.proper.resolve()/'_build/default/lib/proper/ebin'
    prepared = run('cowlib-property-prepare',['erlc','-W0','-DTEST','+export_all','-P','-I',upstream/'include','-pa',proper_ebin,'-o',generated,original])
    if prepared['returncode'] != 0:
        raise ValueError('Cowlib property preprocessing/transform failed')
    listing = generated/(module_name+'.P')
    target = generated/(module_name+'.erl')
    # The listing does not retain erlc's +export_all option. Export only the
    # upstream entry points needed by the probe; property bodies stay untouched.
    listing_text = listing.read_text()
    module = '-module('+module_name+').'
    if listing_text.count(module) != 1:
        raise ValueError('unexpected Cowlib prepared module declaration')
    target.write_text(listing_text.replace(module,module+'\n-export(['+','.join(exports)+']).',1))
    listing.unlink()
    built = run('beam-cowlib-build',['erlc','-W0','-o',beam,target])
    if built['returncode'] != 0:
        raise ValueError('BEAM prepared Cowlib compilation failed')
    report.update(cowlib_pin=pin, cowlib_source_sha256=hashlib.sha256(original.read_bytes()).hexdigest(),
                  cowlib_prepared_sha256=hashlib.sha256(target.read_bytes()).hexdigest(),
                  properties=properties,samples=args.samples,seeds=SEEDS,repetitions=2,
                  shared_generation_sha256=hashlib.sha256((HERE.parent/'proper/generation.py').read_bytes()).hexdigest(),
                  etf_decoder_sha256=hashlib.sha256(_ETF_SOURCE.read_bytes()).hexdigest())
    return {'cowlib_property':dependency}


def results(code,out,err,args):
    if code != 0 or 'Error in process' in out+err:
        raise ValueError('engine crashed or timed out')
    lines = out.splitlines()
    if len(lines) != len(PROPERTIES)*len(SEEDS)+1 or lines[-1] != 'COW_BASE64URL_PROPER_DONE':
        raise ValueError('missing/duplicate/unexpected property result')
    parsed = []
    for line,(name,seed) in zip(lines[:-1],[(n,s) for n in PROPERTIES for s in SEEDS]):
        if not line.startswith('COW_BASE64URL_PROPER:'):
            raise ValueError('unexpected output')
        payload = json.loads(line.split(':',1)[1])
        if type(payload) is not list or any(type(b)is not int or not 0<=b<=255 for b in payload):
            raise ValueError('invalid ETF byte array')
        raw = _etf.etf(bytes(payload))
        # The common decoder preserves Erlang atoms; only the property name is an atom.
        if raw[0] != 'tuple' or len(raw[1]) != 3 or raw[1][0] != ('atom',name):
            raise ValueError('wrong property result')
        actual_seed, value = map(plain,raw[1][1:])
        if actual_seed != seed or type(value) is not tuple or len(value) != 3:
            raise ValueError('wrong seed or malformed result')
        samples,shrinking,example = value
        if type(samples) is not list or len(samples) != args.samples:
            raise ValueError('wrong generated trial count')
        inputs = []
        for sample in samples:
            if type(sample) is not tuple or len(sample) != 3 or not all(type(b)is bytes for b in sample):
                raise ValueError('expected binary samples')
            b,padded,unpadded = sample
            expected = base64.urlsafe_b64encode(b)
            if padded != expected or unpadded != expected.rstrip(b'='):
                raise ValueError('encoding differs from independent Base64url oracle')
            inputs.append(b)
        if b'' not in inputs or {len(b)%3 for b in inputs} != {0,1,2} or len(set(inputs)) <= 1:
            raise ValueError('missing empty input, padding boundary or generated variation')
        if (type(shrinking) is not list or len(shrinking)<2
                or not all(type(b)is bytes for b in shrinking) or not shrinking[0]
                or b'' not in shrinking or example != [b'']):
            raise ValueError('false-property control was not reduced to empty binary')
        parsed.append((name,seed,value))
    for index in range(len(PROPERTIES)):
        group = parsed[index*len(SEEDS):(index+1)*len(SEEDS)]
        if len({repr(x[2][0]) for x in group}) != len(SEEDS):
            raise ValueError('different seeds produced identical traces')
    if args.samples == 1000 and set(b for _,_,v in parsed for sample in v[0] for b in sample[0]) != set(range(256)):
        raise ValueError('default workload missed a byte value')
    return parsed


def completed(code,out,err,args):
    try:
        results(code,out,err,args)
        return True
    except (ValueError,TypeError,IndexError,OverflowError):
        return False


def summary(values,args):
    return {'samples_per_property_seed_repetition':args.samples,'repetitions':2,
            'trials_per_engine':len(PROPERTIES)*len(SEEDS)*2*args.samples,
            'false_shrinking_controls_per_engine':len(PROPERTIES)*len(SEEDS)*2,
            'false_and_true_replays_per_engine':len(PROPERTIES)*len(SEEDS)*4,
            'coverage':[{'property':name,'seed':seed,'count':len(value[0]),
                         'length_min':min(len(x[0]) for x in value[0]),
                         'length_max':max(len(x[0]) for x in value[0]),
                         'length_residues':sorted({len(x[0])%3 for x in value[0]}),
                         'byte_values':len({b for x in value[0] for b in x[0]}),
                         'false_initial_length':len(value[1][0]),
                         'false_shrink_invocations':len(value[1])}
                        for name,seed,value in values]}


if __name__ == '__main__':
    raise SystemExit(proper_native.main(scenario=sys.modules[__name__]))
