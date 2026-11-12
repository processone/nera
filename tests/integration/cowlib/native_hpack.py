#!/usr/bin/env python3
"""Compare cow_hpack's original string properties under native PropEr and BEAM."""
import hashlib
import json
import sys
from native_base64url import (HERE, SEEDS, plain, _etf, configure, validate_args,
                             prepare_module, proper_native)

PROPERTIES = ('prop_str_raw', 'prop_str_huffman')
# HPACK's large Huffman table makes the complete traced workload slower than Base64url.
# A timeout still fails the campaign; never count its partial output as a pass.
RUN_TIMEOUT = 600


def prepare(args, work, report, run, beam):
    report['cowlib_headers'] = {name: hashlib.sha256((args.upstream/'src'/name).read_bytes()).hexdigest()
                               for name in ('cow_hpack_common.hrl', 'cow_hpack_dec_huffman_lookup.hrl')}
    return prepare_module(args, work, report, run, beam, 'cow_hpack', PROPERTIES,
                          tuple(name+'/0' for name in PROPERTIES)+('enc_str/2', 'dec_str/1'))


def driver(args):
    seeds = '['+','.join('{'+','.join(map(str, seed))+'}' for seed in SEEDS)+']'
    properties = '['+','.join(PROPERTIES)+']'
    return r'''-module(proper_probe).
-export([start/0]).
start() ->
    lists:foreach(fun(Name) ->
        lists:foreach(fun(Seed) ->
            Result = run(Name,Seed),
            Result = run(Name,Seed),
            io:format("COW_HPACK_RESULT:~w~n",[binary_to_list(term_to_binary({Name,Seed,Result}))])
        end,SEED_LIST)
    end,PROPERTY_NAMES),
    io:format("COW_HPACK_DONE~n").
seeded(Seed,Property) ->
    proper:setup(fun() ->
        rand:seed(exsplus,Seed), put(hpack_trace,[]), fun() -> ok end
    end,Property).
record(Value) -> put(hpack_trace,[Value|get(hpack_trace)]),ok.
trace() -> lists:reverse(erase(hpack_trace)).
run(Name,Seed) ->
    Original = apply(cow_hpack,Name,[]),
    {forall,Type,Predicate} = Original,
    Mode = case Name of prop_str_raw -> no_huffman; prop_str_huffman -> huffman end,
    Observed = proper:forall(Type,fun(Input) ->
        Wire = iolist_to_binary(cow_hpack:enc_str(Input,Mode)),
        Decoded = cow_hpack:dec_str(Wire),
        Verdict = Predicate(Input),
        record({Input,Wire,Decoded,Verdict}), Verdict
    end),
    true = proper:quickcheck(seeded(Seed,Observed),
        [quiet,{numtests,SAMPLE_COUNT},{start_size,0},{max_size,256}]),
    Samples = trace(), SAMPLE_COUNT = length(Samples),
    False = proper:forall(Type,fun(Input) -> record(Input), Predicate(Input) andalso false end),
    false = proper:quickcheck(seeded(Seed,False),
        [quiet,{numtests,100},{start_size,20},{max_size,40},{max_shrinks,1000}]),
    [<<>>] = proper:counterexample(),
    Shrinking = trace(), true = hd(Shrinking) =/= <<>>,
    false = proper:check(seeded(Seed,False),[<<>>],[quiet]),
    [<<>>] = trace(),
    true = proper:check(seeded(Seed,Original),[<<>>],[quiet]),
    [] = trace(),
    {Samples,Shrinking,[<<>>]}.
'''.replace('SAMPLE_COUNT', str(args.samples)).replace('SEED_LIST', seeds).replace('PROPERTY_NAMES', properties)


def string_payload(wire, huffman):
    """Check RFC 7541's 7-bit length prefix, flag and exact payload length."""
    if not wire or wire[0] >> 7 != huffman:
        raise ValueError('wrong string flag')
    length, offset = wire[0] & 127, 1
    if length == 127:
        shift = 0
        while True:
            if offset >= len(wire) or shift > 63:
                raise ValueError('incomplete or excessive string length')
            byte = wire[offset]
            offset += 1
            length += (byte & 127) << shift
            if byte < 128:
                break
            shift += 7
    if len(wire)-offset != length:
        raise ValueError('wrong string payload length')
    return wire[offset:]


def results(code, out, err, args):
    if code != 0 or 'Error in process' in out+err:
        raise ValueError('engine crashed or timed out')
    lines = out.splitlines()
    expected = [(name, seed) for name in PROPERTIES for seed in SEEDS]
    if len(lines) != len(expected)+1 or lines[-1] != 'COW_HPACK_DONE':
        raise ValueError('missing, duplicate or unexpected property result')
    parsed = []
    for line, (name, seed) in zip(lines, expected):
        if not line.startswith('COW_HPACK_RESULT:'):
            raise ValueError('missing result marker')
        payload = json.loads(line.split(':', 1)[1])
        if type(payload) is not list or any(type(x) is not int or not 0 <= x <= 255 for x in payload):
            raise ValueError('invalid ETF bytes')
        raw = _etf.etf(bytes(payload))
        if raw[0] != 'tuple' or len(raw[1]) != 3 or raw[1][0] != ('atom', name):
            raise ValueError('wrong property envelope')
        actual_seed = plain(raw[1][1])
        result = raw[1][2]
        if actual_seed != seed or result[0] != 'tuple' or len(result[1]) != 3:
            raise ValueError('wrong seed or missing results')
        samples, shrinking, example = result[1]
        if samples[0] != 'list' or len(samples[1]) != args.samples:
            raise ValueError('wrong trial count')
        inputs = []
        for sample in samples[1]:
            if sample[0] != 'tuple' or len(sample[1]) != 4:
                raise ValueError('malformed sample')
            value, encoded, decoded, verdict = sample[1]
            if value[0] != 'binary' or encoded[0] != 'binary' or verdict != ('atom', 'true'):
                raise ValueError('invalid input, wire or original verdict')
            if decoded != ('tuple', (value, ('binary', ''))):
                raise ValueError('incorrect decoded value or remainder')
            data, wire = bytes.fromhex(value[1]), bytes.fromhex(encoded[1])
            body = string_payload(wire, name == 'prop_str_huffman')
            if name == 'prop_str_raw' and body != data:
                raise ValueError('raw string differs from independent wire oracle')
            inputs.append(data)
        if b'' not in inputs or len(set(inputs)) <= 1:
            raise ValueError('missing empty input or generated variation')
        if args.samples == 1000 and (max(map(len, inputs)) < 127
                                    or set(b for data in inputs for b in data) != set(range(256))):
            raise ValueError('default workload missed long strings or byte values')
        if (shrinking[0] != 'list' or len(shrinking[1]) < 2
                or any(x[0] != 'binary' for x in shrinking[1])
                or shrinking[1][0] == ('binary', '')
                or ('binary', '') not in shrinking[1]
                or example != ('list', (('binary', ''),))):
            raise ValueError('false control did not shrink to empty binary')
        parsed.append((name, seed, result))
    for name in PROPERTIES:
        group = [result[1][0] for n, _, result in parsed if n == name]
        if len(set(map(repr, group))) != len(SEEDS):
            raise ValueError('distinct seeds produced identical traces')
    return parsed


def completed(code, out, err, args):
    try:
        results(code, out, err, args)
        return True
    except (ValueError, TypeError, IndexError, OverflowError):
        return False


def summary(values, args):
    coverage = []
    for name, seed, result in values:
        samples, shrinking, _ = result[1]
        inputs = [bytes.fromhex(x[1][0][1]) for x in samples[1]]
        coverage.append({'property': name, 'seed': seed, 'samples': len(inputs),
                         'length_min': min(map(len, inputs)), 'length_max': max(map(len, inputs)),
                         'byte_values': len(set(b for data in inputs for b in data)),
                         'sample_trace_sha256': hashlib.sha256(repr(samples).encode()).hexdigest(),
                         'shrink_invocations': len(shrinking[1])})
    return {'properties': PROPERTIES, 'trials_per_property_per_engine': args.samples*len(SEEDS)*2,
            'trials_per_engine': len(PROPERTIES)*args.samples*len(SEEDS)*2,
            'repetitions': 2, 'false_controls_per_engine': len(PROPERTIES)*len(SEEDS)*2,
            'false_and_true_replays_per_engine': len(PROPERTIES)*len(SEEDS)*4,
            'comparison': 'exact typed input/wire/decoded/verdict/shrink traces; no semantic normalization',
            'oracle': 'independent raw wire and string length/flag checks; no independent Huffman oracle',
            'coverage': coverage}


if __name__ == '__main__':
    raise SystemExit(proper_native.main(scenario=sys.modules[__name__]))
