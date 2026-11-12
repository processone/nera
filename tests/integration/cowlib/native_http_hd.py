#!/usr/bin/env python3
"""Compare original cow_http_hd properties under native PropEr and BEAM."""
import hashlib
import json
import sys
from native_base64url import HERE, SEEDS, _etf, configure as common_configure, validate_args as common_validate_args, prepare_module, proper_native

PROPERTIES = ('prop_parse_accept', 'prop_parse_accept_charset', 'prop_parse_accept_encoding', 'prop_parse_accept_language', 'prop_parse_access_control_request_headers', 'prop_parse_allow', 'prop_parse_cache_control', 'prop_parse_connection', 'prop_parse_content_language', 'prop_parse_content_length', 'prop_parse_content_range', 'prop_parse_content_type', 'prop_parse_etag', 'prop_parse_expect', 'prop_parse_host', 'prop_parse_if_match', 'prop_parse_max_forwards', 'prop_parse_origin', 'prop_parse_range', 'prop_parse_sec_websocket_extensions', 'prop_parse_sec_websocket_protocol_resp', 'prop_parse_sec_websocket_version_req', 'prop_parse_sec_websocket_version_resp', 'prop_parse_te', 'prop_parse_transfer_encoding', 'prop_parse_upgrade')


def configure(parser):
    common_configure(parser)
    parser.set_defaults(samples=100)
    parser.add_argument('--max-size',type=int,default=10,choices=range(1,51),metavar='1..50')
    parser.add_argument('--properties',nargs='+',choices=PROPERTIES,default=PROPERTIES)


def validate_args(args,parser):
    common_validate_args(args,parser)
    if len(set(args.properties))!=len(args.properties):
        parser.error('--properties must contain distinct names')


def prepare(args,work,report,run,beam):
    deps = prepare_module(args,work,report,run,beam,'cow_http_hd',PROPERTIES,
                          tuple(name+'/0' for name in PROPERTIES))
    report.update(properties=args.properties,upstream_properties=PROPERTIES,max_size=args.max_size,
                  cowlib_preparation_sha256=hashlib.sha256((HERE/'native_base64url.py').read_bytes()).hexdigest())
    return deps


def driver(args):
    names = '['+','.join(args.properties)+']'
    return r'''-module(proper_probe).
-export([start/0]).
start() ->
    lists:foreach(fun(Name) ->
        lists:foreach(fun(Seed) ->
            {A,B,C} = Seed,
            io:format("COW_HD_START:~w:~w,~w,~w~n",[Name,A,B,C]),
            Result = run(Name,Seed),
            Result = run(Name,Seed),
            io:format("COW_HD_RESULT:~w~n",[binary_to_list(term_to_binary({Name,Seed,Result}))])
        end,[{101,202,303},{7,11,13},{999,1001,2027}])
    end,PROPERTY_NAMES),
    io:format("COW_HD_DONE~n").
seeded(Seed,Property) ->
    proper:setup(fun() ->
        rand:seed(exsplus,Seed), put(hd_trace,[]), fun() -> ok end
    end,Property).
record(Value) -> put(hd_trace,[Value|get(hd_trace)]),ok.
trace() -> lists:reverse(erase(hd_trace)).
run(Name,Seed) ->
    Original = apply(cow_http_hd,Name,[]),
    {forall,Type,Predicate} = Original,
    Observed = proper:forall(Type,fun(Input) ->
        Result = Predicate(Input), record({Input,Result}), Result
    end),
    true = proper:quickcheck(seeded(Seed,Observed),
        [quiet,{numtests,SAMPLE_COUNT},{start_size,1},{max_size,MAX_SIZE}]),
    Samples = trace(), SAMPLE_COUNT = length(Samples),
    False = proper:forall(Type,fun(Input) -> record(Input), Predicate(Input) andalso false end),
    false = proper:quickcheck(seeded(Seed,False),
        [quiet,{numtests,1},{start_size,1},{max_size,MAX_SIZE},{max_shrinks,100}]),
    [Minimum] = Counterexample = proper:counterexample(),
    Shrinking = trace(),
    false = proper:check(seeded(Seed,False),Counterexample,[quiet]),
    [Minimum] = trace(),
    true = proper:check(seeded(Seed,Original),Counterexample,[quiet]),
    [] = trace(),
    {Samples,Shrinking,Counterexample}.
'''.replace('PROPERTY_NAMES',names).replace('SAMPLE_COUNT',str(args.samples)).replace('MAX_SIZE',str(args.max_size))


def results(code,out,err,args):
    if not args.properties or len(set(args.properties))!=len(args.properties) or any(n not in PROPERTIES for n in args.properties):
        raise ValueError('invalid property selection')
    if code!=0 or 'Error in process' in out+err:
        raise ValueError('engine crashed or timed out')
    expected = [(name,seed) for name in args.properties for seed in SEEDS]
    lines = out.splitlines()
    if len(lines)!=2*len(expected)+1 or lines[-1]!='COW_HD_DONE':
        raise ValueError('missing, duplicate or unexpected property result')
    parsed = []
    for i,(name,seed) in enumerate(expected):
        if lines[2*i]!='COW_HD_START:'+name+':'+','.join(map(str,seed)):
            raise ValueError('wrong property/seed progress marker')
        line = lines[2*i+1]
        if not line.startswith('COW_HD_RESULT:'):
            raise ValueError('missing result marker')
        payload = json.loads(line.split(':',1)[1])
        if type(payload)is not list or any(type(x)is not int or not 0<=x<=255 for x in payload):
            raise ValueError('invalid ETF bytes')
        value = _etf.etf(bytes(payload))
        if value[0]!='tuple' or len(value[1])!=3:
            raise ValueError('invalid result envelope')
        actual_name,actual_seed,result = value[1]
        if actual_name!=('atom',name) or actual_seed!=('tuple',tuple(('integer',x) for x in seed)):
            raise ValueError('wrong property or seed')
        if result[0]!='tuple' or len(result[1])!=3:
            raise ValueError('missing generation/control results')
        samples,shrinking,example = result[1]
        if samples[0]!='list' or len(samples[1])!=args.samples:
            raise ValueError('wrong trial count')
        inputs = []
        for sample in samples[1]:
            if sample[0]!='tuple' or len(sample[1])!=2 or sample[1][1]!=('atom','true'):
                raise ValueError('original property did not return true')
            inputs.append(sample[1][0])
        if len(set(map(repr,inputs)))<=1:
            raise ValueError('missing generated variation')
        if (shrinking[0]!='list' or not shrinking[1] or example[0]!='list' or len(example[1])!=1
                or example[1][0] not in shrinking[1]):
            raise ValueError('missing false-control counterexample/trace')
        parsed.append((name,seed,result))
    for name in args.properties:
        group = [result[1][0] for n,_,result in parsed if n==name]
        if len(set(map(repr,group)))!=len(SEEDS):
            raise ValueError('distinct seeds produced identical traces')
    return parsed


def completed(code,out,err,args):
    try:
        results(code,out,err,args)
        return True
    except (ValueError,TypeError,IndexError,OverflowError):
        return False


def summary(values,args):
    return {'properties':list(args.properties),'trials_per_property_per_engine':args.samples*len(SEEDS)*2,
            'trials_per_engine':len(args.properties)*args.samples*len(SEEDS)*2,
            'repetitions':2,'false_controls_per_engine':len(args.properties)*len(SEEDS)*2,
            'false_and_true_replays_per_engine':len(args.properties)*len(SEEDS)*4,
            'max_shrinks':100,'minimality_claim':False,
            'coverage':[{'property':name,'seed':seed,'samples':len(result[1][0][1]),
                         'sample_trace_sha256':hashlib.sha256(repr(result[1][0]).encode()).hexdigest(),
                         'shrink_invocations':len(result[1][1][1]),
                         'changed_counterexample':result[1][1][1][0]!=result[1][2][1][0],
                         'counterexample_sha256':hashlib.sha256(repr(result[1][2]).encode()).hexdigest()}
                        for name,seed,result in values]}


if __name__ == '__main__':
    raise SystemExit(proper_native.main(scenario=sys.modules[__name__]))
