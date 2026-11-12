"""Project-owned native PropEr generation/shrinking probe (no dependency sources)."""
import importlib.util
import json
from pathlib import Path

_ETF_SOURCE = Path(__file__).resolve().parents[1] / "thoas/differential/runner.py"
_spec = importlib.util.spec_from_file_location("proper_etf", _ETF_SOURCE)
_etf = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_etf)

SEEDS = ((101, 202, 303), (7, 11, 13), (999, 1001, 2027))
DRIVER = r'''-module(proper_probe).
-export([start/0]).
start() ->
    lists:foreach(fun(Seed) ->
        A = run(Seed),
        A = run(Seed),
        io:format("PROPER_GENERATION:~w~n", [binary_to_list(term_to_binary({Seed,A}))])
    end, [{101,202,303},{7,11,13},{999,1001,2027}]),
    io:format("PROPER_GENERATION_DONE~n").

seeded(Seed, Property) ->
    proper:setup(fun() ->
        rand:seed(exsplus, Seed),
        put(probe_trace, []),
        fun() -> ok end
    end, Property).

record(Value) -> put(probe_trace, [Value|get(probe_trace)]), ok.
trace() -> lists:reverse(erase(probe_trace)).
options() -> [quiet, {numtests,100}, {start_size,20}, {max_size,40}, {max_shrinks,1000}].

run(Seed) ->
    Type = {proper_types:integer(-100,100),
            proper_types:list(proper_types:integer(0,255)), proper_types:binary()},
    Valid = proper:forall(Type, fun({I,L,B}=Value) ->
        record(Value),
        is_integer(I) andalso I >= -100 andalso I =< 100
        andalso is_list(L) andalso lists:all(fun(X) -> X >= 0 andalso X =< 255 end, L)
        andalso is_binary(B) andalso list_to_binary(binary_to_list(B)) =:= B
    end),
    true = proper:quickcheck(seeded(Seed,Valid), options()),
    Generated = trace(),
    100 = length(Generated),
    true = length(lists:usort(Generated)) > 1,
    Int = failure(Seed, proper_types:integer(0,100), fun(X) -> X < 10 end, [10]),
    List = failure(Seed, proper_types:list(proper_types:integer(0,100)),
                   fun(L) -> length(L) < 3 end, [[0,0,0]]),
    Binary = failure(Seed, proper_types:binary(), fun(B) -> byte_size(B) < 3 end, [<<0,0,0>>]),
    {Generated,Int,List,Binary}.

failure(Seed, Type, Predicate, Expected) ->
    Property = proper:forall(Type, fun(X) -> record(X), Predicate(X) end),
    false = proper:quickcheck(seeded(Seed,Property), options()),
    Expected = proper:counterexample(),
    Shrinking = trace(),
    %% Prove the minimized example differs from the original generated failure.
    [Initial|_] = [X || X <- Shrinking, not Predicate(X)],
    [Minimum] = Expected,
    true = Initial =/= Minimum,
    false = proper:check(seeded(Seed,Property), Expected, [quiet]),
    [Minimum] = trace(),
    true = proper:check(seeded(Seed,Property), [case Minimum of
        10 -> 9;
        [0,0,0] -> [0,0];
        <<0,0,0>> -> <<0,0>>
    end], [quiet]),
    _ = trace(),
    {Shrinking,Expected}.
'''


def plain(term):
    kind, value = term
    if kind == 'integer':
        return value
    if kind == 'binary':
        return bytes.fromhex(value)
    if kind == 'tuple':
        return tuple(plain(x) for x in value)
    if kind == 'list':
        return [plain(x) for x in value]
    raise ValueError('unexpected generated type')


def valid_integer(x, low, high):
    return type(x) is int and low <= x <= high


def valid_list(x, high):
    return type(x) is list and all(valid_integer(i, 0, high) for i in x)


def validate(value, seed):
    if type(value) is not tuple or len(value) != 2 or value[0] != seed:
        raise ValueError('wrong seed or result envelope')
    result = value[1]
    if type(result) is not tuple or len(result) != 4:
        raise ValueError('missing property results')
    generated, *failures = result
    if type(generated) is not list or len(generated) != 100:
        raise ValueError('expected 100 real generated instances')
    for x in generated:
        if (type(x) is not tuple or len(x) != 3
                or not valid_integer(x[0], -100, 100)
                or not valid_list(x[1], 255) or type(x[2]) is not bytes):
            raise ValueError('generated instance outside declared type')
    if len(set(map(repr, generated))) <= 1:
        raise ValueError('generator produced no variation')
    specs = [(lambda x: valid_integer(x, 0, 100), lambda x: x < 10, 10),
             (lambda x: valid_list(x, 100), lambda x: len(x) < 3, [0, 0, 0]),
             (lambda x: type(x) is bytes, lambda x: len(x) < 3, bytes(3))]
    for failure, (valid, predicate, minimum) in zip(failures, specs):
        if type(failure) is not tuple or len(failure) != 2:
            raise ValueError('missing shrink trace/counterexample')
        trace, example = failure
        if type(trace) is not list or not trace or not all(valid(x) for x in trace):
            raise ValueError('invalid shrink candidates')
        rejected = [x for x in trace if not predicate(x)]
        if (example != [minimum] or not rejected or rejected[0] == minimum
                or minimum not in rejected or not any(predicate(x) for x in trace)):
            raise ValueError('missing real reduction or wrong minimum')
    return value


def results(code, out, err):
    if code != 0 or 'Error in process' in out + err:
        raise ValueError('engine failed')
    lines = out.splitlines()
    if len(lines) != 4 or lines[-1] != 'PROPER_GENERATION_DONE':
        raise ValueError('missing, duplicate or unexpected output')
    parsed = []
    for line, seed in zip(lines[:-1], SEEDS):
        prefix = 'PROPER_GENERATION:'
        if not line.startswith(prefix):
            raise ValueError('missing seed result')
        payload = json.loads(line[len(prefix):])
        if (type(payload) is not list or not payload
                or any(type(x) is not int or not 0 <= x <= 255 for x in payload)):
            raise ValueError('invalid ETF byte array')
        parsed.append(validate(plain(_etf.etf(bytes(payload))), seed))
    if len({repr(value[1][0]) for value in parsed}) != len(SEEDS):
        raise ValueError('different seeds produced identical generation traces')
    return parsed


def completed(code, out, err):
    try:
        results(code, out, err)
        return True
    except (ValueError, TypeError, IndexError, OverflowError):
        return False
