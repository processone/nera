#!/usr/bin/env python3
"""Run original cow_date property with native PropEr and BEAM."""
import json
import re
import sys
from native_base64url import HERE, SEEDS, plain, _etf, configure, validate_args, proper_native


def prepare(args,work,report,run,beam):
    from native_base64url import prepare_module
    import hashlib
    dependencies = prepare_module(args,work,report,run,beam,'cow_date',('prop_http_date',),('prop_http_date/0',))
    report['cowlib_preparation_sha256'] = hashlib.sha256((HERE/'native_base64url.py').read_bytes()).hexdigest()
    return dependencies


def driver(args):
    return r'''-module(proper_probe).
-export([start/0]).
start() ->
    lists:foreach(fun(Seed) ->
        Result = run(Seed),
        Result = run(Seed),
        io:format("COW_DATE_PROPER:~w~n",[binary_to_list(term_to_binary({Seed,Result}))])
    end,[{101,202,303},{7,11,13},{999,1001,2027}]),
    io:format("COW_DATE_PROPER_DONE~n").
seeded(Seed,Property) ->
    proper:setup(fun() ->
        rand:seed(exsplus,Seed), put(date_trace,[]), fun() -> ok end
    end,Property).
record(Value) -> put(date_trace,[Value|get(date_trace)]),ok.
trace() -> lists:reverse(erase(date_trace)).
run(Seed) ->
    Original = cow_date:prop_http_date(),
    {forall,Type,Predicate} = Original,
    Observed = proper:forall(Type,fun({_,Bin}=Input) ->
        record({Input,cow_date:parse_date(Bin)}), Predicate(Input)
    end),
    true = proper:quickcheck(seeded(Seed,Observed),
        [quiet,{numtests,SAMPLE_COUNT},{start_size,1},{max_size,50}]),
    Samples = trace(),
    SAMPLE_COUNT = length(Samples),
    False = proper:forall(Type,fun(Input) -> record(Input), Predicate(Input) andalso false end),
    false = proper:quickcheck(seeded(Seed,False),
        [quiet,{numtests,100},{start_size,1},{max_size,50},{max_shrinks,1000}]),
    [Minimum] = Counterexample = proper:counterexample(),
    Shrinking = trace(),
    true = hd(Shrinking) =/= Minimum,
    false = proper:check(seeded(Seed,False),Counterexample,[quiet]),
    [Minimum] = trace(),
    true = proper:check(seeded(Seed,Original),Counterexample,[quiet]),
    [] = trace(),
    {Samples,Shrinking,Counterexample}.
'''.replace('SAMPLE_COUNT',str(args.samples))


MONTHS = ('Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec')
SHORT_DAYS = ('Mon','Tue','Wed','Thu','Fri','Sat','Sun')
LONG_DAYS = ('Monday','Tuesday','Wednesday','Thursday','Friday','Saturday','Sunday')


def parse_wire(wire):
    if type(wire) is not bytes:
        raise ValueError('expected date binary')
    text = wire.decode('ascii')
    fix = re.fullmatch(r'([A-Za-z]+), (\d{2}) ([A-Za-z]+) (\d{4}) (\d{2}):(\d{2}):(\d{2}) GMT',text)
    rfc = re.fullmatch(r'([A-Za-z]+), (\d{2})-([A-Za-z]+)-(\d{2}) (\d{2}):(\d{2}):(\d{2}) GMT',text)
    asc = re.fullmatch(r'([A-Za-z]+) ([A-Za-z]+) ([ 0-9][0-9]) (\d{2}):(\d{2}):(\d{2}) (\d{4})',text)
    if fix or rfc:
        match = fix or rfc
        weekday,day,month,year,hour,minute,second = match.groups()
        if weekday not in (SHORT_DAYS if fix else LONG_DAYS):
            raise ValueError('invalid weekday')
        year = int(year)
        if rfc:
            year += 1900 if year > 50 else 2000
        label = 'fixdate' if fix else 'rfc850'
    elif asc:
        weekday,month,day,hour,minute,second,year = asc.groups()
        if weekday not in SHORT_DAYS or day != f'{int(day):2d}':
            raise ValueError('invalid asctime weekday or day padding')
        year = int(year)
        label = 'asctime'
    else:
        raise ValueError('unknown HTTP date format')
    if month not in MONTHS:
        raise ValueError('invalid month')
    date = ((year,MONTHS.index(month)+1,int(day)),(int(hour),int(minute),int(second)))
    validate_date(date)
    return label,date


def validate_date(date):
    if (type(date) is not tuple or len(date)!=2 or
            any(type(x)is not tuple or len(x)!=3 for x in date)):
        raise ValueError('invalid date structure')
    bounds = ((1951,2050),(1,12),(1,31),(0,23),(0,59),(0,60))
    if any(type(x)is not int or not low<=x<=high for x,(low,high) in zip(date[0]+date[1],bounds)):
        raise ValueError('date outside upstream component ranges')


def validate_input(value):
    if type(value)is not tuple or len(value)!=2:
        raise ValueError('invalid generated input')
    expected,wire = value
    validate_date(expected)
    label,actual = parse_wire(wire)
    if actual != expected:
        raise ValueError('generated date does not match independent wire oracle')
    return label


def results(code,out,err,args):
    if code != 0 or 'Error in process' in out+err:
        raise ValueError('engine crashed or timed out')
    lines = out.splitlines()
    if len(lines)!=len(SEEDS)+1 or lines[-1]!='COW_DATE_PROPER_DONE':
        raise ValueError('missing/duplicate/unexpected output')
    parsed = []
    for line,seed in zip(lines[:-1],SEEDS):
        if not line.startswith('COW_DATE_PROPER:'):
            raise ValueError('wrong result marker')
        payload = json.loads(line.split(':',1)[1])
        if type(payload)is not list or any(type(x)is not int or not 0<=x<=255 for x in payload):
            raise ValueError('invalid ETF byte array')
        value = plain(_etf.etf(bytes(payload)))
        if type(value)is not tuple or len(value)!=2 or value[0]!=seed:
            raise ValueError('wrong seed/result envelope')
        result = value[1]
        if type(result)is not tuple or len(result)!=3:
            raise ValueError('missing generation/shrink results')
        samples,shrinking,example = result
        if type(samples)is not list or len(samples)!=args.samples:
            raise ValueError('wrong generated trial count')
        formats = set()
        for sample in samples:
            if type(sample)is not tuple or len(sample)!=2:
                raise ValueError('invalid observed sample')
            source,actual = sample
            formats.add(validate_input(source))
            validate_date(actual)
            if source[0]!=actual:
                raise ValueError('incorrect parsed date')
        if formats!={'fixdate','rfc850','asctime'} or len({repr(x) for x in samples})<=1:
            raise ValueError('missing format coverage/variation')
        if (type(shrinking)is not list or len(shrinking)<2 or type(example)is not list or len(example)!=1):
            raise ValueError('invalid false-control reduction')
        for candidate in shrinking:
            validate_input(candidate)
        validate_input(example[0])
        if example[0] not in shrinking or shrinking[0]==example[0]:
            raise ValueError('no actual reduction')
        if example[0][0]!=((1951,1,1),(0,0,0)):
            raise ValueError('false control did not reach minimum component values')
        parsed.append(value)
    if len({repr(x[1][0]) for x in parsed})!=len(SEEDS):
        raise ValueError('distinct seeds produced identical traces')
    return parsed


def completed(code,out,err,args):
    try:
        results(code,out,err,args)
        return True
    except (ValueError,TypeError,IndexError,OverflowError,UnicodeError):
        return False


def summary(values,args):
    coverage = []
    for seed,(samples,shrink,example) in values:
        formats = {}
        for (date,wire),actual in samples:
            label,_ = parse_wire(wire)
            group = formats.setdefault(label,{'count':0,'components':[set() for _ in range(6)]})
            group['count'] += 1
            for bucket,value in zip(group['components'],date[0]+date[1]):bucket.add(value)
        coverage.append({'seed':seed,'formats':{name:{'count':x['count'],'components':dict(zip(
                         ('years','months','days','hours','minutes','seconds'),[sorted(v) for v in x['components']]))} for name,x in formats.items()},
                         'false_initial':repr(shrink[0]),'false_minimum':repr(example[0]),'shrink_invocations':len(shrink)})
    return {'trials_per_engine':len(SEEDS)*2*args.samples,'trials_per_seed_repetition':args.samples,
            'repetitions':2,'false_shrinking_controls_per_engine':len(SEEDS)*2,
            'false_and_true_replays_per_engine':len(SEEDS)*4,'coverage':coverage}


if __name__ == '__main__':
    raise SystemExit(proper_native.main(scenario=sys.modules[__name__]))
