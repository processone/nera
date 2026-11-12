import copy
import json
import unittest
from generation import completed, results, validate, SEEDS


def fixture(seed):
    return (seed, ([(i - 50, [i], bytes([i])) for i in range(100)],
                   ([83, 0, 9, 10], [10]),
                   ([[8, 9, 4, 5], [], [0, 0, 0]], [[0, 0, 0]]),
                   ([bytes([1, 2, 3, 4]), b'', bytes(3)], [bytes(3)])))


def encode(value):
    # Tiny project-owned ETF fixture writer, not used by either engine.
    if type(value) is int:
        return bytes([98]) + value.to_bytes(4, 'big', signed=True)
    if type(value) is bytes:
        return bytes([109]) + len(value).to_bytes(4, 'big') + value
    if type(value) is tuple:
        return bytes([104, len(value)]) + b''.join(map(encode, value))
    if type(value) is list:
        return bytes([108]) + len(value).to_bytes(4, 'big') + b''.join(map(encode, value)) + bytes([106])
    raise TypeError(value)


def output():
    values = [fixture(seed) for seed in SEEDS]
    for i, (_, result) in enumerate(values):
        # Distinct, in-range traces for each seed.
        result[0][0] = (-50 + i, [i], bytes([i]))
    return ''.join('PROPER_GENERATION:' + json.dumps(list(bytes([131]) + encode(v))) + '\n'
                   for v in values) + 'PROPER_GENERATION_DONE\n'


class GenerationProtocolTests(unittest.TestCase):
    def test_accepts_exact_three_seed_results(self):
        good = output()
        self.assertTrue(completed(0, good, ''))
        self.assertEqual(len(results(0, good, '')), 3)

    def test_rejects_crash_timeout_missing_duplicate_and_invalid_payload(self):
        good = output()
        for code, out, err in [
            (1, good, ''), (None, good, ''), (0, '', ''),
            (0, 'PROPER_GENERATION_DONE\n', ''),
            (0, good + 'PROPER_GENERATION_DONE\n', ''),
            (0, '\n'.join(good.splitlines()[1:]) + '\n', ''),
            (0, good, 'Error in process <0.1.0>'),
            (0, good.replace('[131,', '[true,', 1), ''),
            (0, good.replace('[131,', '[999,', 1), ''),
            (0, good.replace('PROPER_GENERATION:', 'UNEXPECTED:', 1), ''),
            (0, good.replace('[131,', '[132,', 1), ''),
            (0, good.replace('PROPER_GENERATION:[', 'PROPER_GENERATION:[-1,', 1), ''),
            (0, good.replace('PROPER_GENERATION:[', 'PROPER_GENERATION:invalid[', 1), ''),
        ]:
            self.assertFalse(completed(code, out, err), (code, out[:100], err))

    def test_rejects_same_generation_trace_for_distinct_seeds(self):
        same = ''.join('PROPER_GENERATION:' + json.dumps(list(bytes([131]) + encode(fixture(seed)))) + '\n'
                       for seed in SEEDS) + 'PROPER_GENERATION_DONE\n'
        self.assertFalse(completed(0, same, ''))

    def test_checks_generated_values_and_actual_reduction(self):
        validate(fixture(SEEDS[0]), SEEDS[0])
        mutations = []
        value = copy.deepcopy(fixture(SEEDS[0]))
        value[1][0].pop()
        mutations.append(value)
        value = copy.deepcopy(fixture(SEEDS[0]))
        value[1][0][0] = (101, [], b'')
        mutations.append(value)
        value = copy.deepcopy(fixture(SEEDS[0]))
        value[1][1][1][0] = 11
        mutations.append(value)
        value = copy.deepcopy(fixture(SEEDS[0]))
        value[1][1][0][0] = 10
        mutations.append(value)
        value = copy.deepcopy(fixture(SEEDS[0]))
        value[1][2][0][0] = [101, 0, 0]
        mutations.append(value)
        value = copy.deepcopy(fixture(SEEDS[0]))
        value[1][0][:] = [value[1][0][0]] * 100
        mutations.append(value)
        value = copy.deepcopy(fixture(SEEDS[0]))
        value[1][3][1][0] = bytes([1, 0, 0])
        mutations.append(value)
        for value in mutations:
            with self.assertRaises(ValueError):
                validate(value, SEEDS[0])
        with self.assertRaises(ValueError):
            validate(fixture(SEEDS[0]), SEEDS[1])


if __name__ == '__main__':
    unittest.main()
