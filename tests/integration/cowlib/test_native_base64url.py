import base64
import copy
import json
from types import SimpleNamespace
import unittest
from native_base64url import completed, SEEDS, PROPERTIES


def encode(value):
    if type(value) is int:
        return bytes([98]) + value.to_bytes(4, 'big', signed=True)
    if type(value) is bytes:
        return bytes([109]) + len(value).to_bytes(4, 'big') + value
    if type(value) is str:
        b = value.encode()
        return bytes([119,len(b)]) + b
    if type(value) is tuple:
        return bytes([104,len(value)]) + b''.join(map(encode,value))
    if type(value) is list:
        return bytes([108]) + len(value).to_bytes(4, 'big') + b''.join(map(encode,value)) + bytes([106])
    raise TypeError(value)


def record(name, seed):
    inputs = [b'', bytes([seed[0] % 256]), b'xy', b'xyz']
    samples = [(b,base64.urlsafe_b64encode(b),base64.urlsafe_b64encode(b).rstrip(b'=')) for b in inputs]
    return (name,seed,(samples,[b'abcd',b''],[b'']))


def output(values=None):
    if values is None:
        values = [record(name,seed) for name in PROPERTIES for seed in SEEDS]
    return ''.join('COW_BASE64URL_PROPER:' + json.dumps(list(bytes([131])+encode(value))) + '\n'
                   for value in values) + 'COW_BASE64URL_PROPER_DONE\n'


class NativeBase64urlTests(unittest.TestCase):
    def test_accepts_complete_native_results(self):
        self.assertTrue(completed(0, output(), '', SimpleNamespace(samples=4)))

    def test_rejects_failed_incomplete_duplicate_or_corrupt_protocol(self):
        good = output()
        lines = good.splitlines()
        for code,out,err in [
            (1,good,''),(None,good,''),(0,'',''),
            (0,'COW_BASE64URL_PROPER_DONE\n',''),
            (0,good+'extra\n',''),
            (0,'\n'.join(lines[:-1])+'\n',''),
            (0,'\n'.join([lines[0],lines[0],*lines[2:]])+'\n',''),
            (0,good,'Error in process <0.1.0>'),
            (0,good.replace('[131,','[true,',1),''),
            (0,good.replace('[131,','[256,',1),''),
            (0,good.replace('[131,','[0,',1),''),
            (0,good.replace('COW_BASE64URL_PROPER:','UNEXPECTED:',1),''),
        ]:
            self.assertFalse(completed(code,out,err,SimpleNamespace(samples=4)),(code,out[:80],err))

    def test_checks_oracle_count_padding_boundaries_and_shrinking(self):
        args = SimpleNamespace(samples=4)
        original = [record(name,seed) for name in PROPERTIES for seed in SEEDS]
        mutations = []
        values = copy.deepcopy(original)
        values[0][2][0].pop()
        mutations.append(values)
        values = copy.deepcopy(original)
        b,pad,_ = values[0][2][0][1]
        values[0][2][0][1] = (b,pad,pad)  # Wrong no-padding encoding.
        mutations.append(values)
        values = copy.deepcopy(original)
        values[0][2][0][0] = (b'',b'AAAA',b'AAAA')
        mutations.append(values)
        values = copy.deepcopy(original)
        values[0][2][1][0] = b''  # No real reduction.
        mutations.append(values)
        values = copy.deepcopy(original)
        values[0][2][2][0] = b'x'  # Wrong minimum.
        mutations.append(values)
        values = copy.deepcopy(original)
        values[0] = (values[0][0],SEEDS[1],values[0][2])
        mutations.append(values)
        values = copy.deepcopy(original)
        values[0] = (PROPERTIES[1],values[0][1],values[0][2])
        mutations.append(values)
        values = copy.deepcopy(original)
        values[0][2][0][:] = [(b'xxx',b'eHh4',b'eHh4')]*4
        mutations.append(values)
        values = copy.deepcopy(original)
        values[0][2][0][0] = (1,b'',b'')
        mutations.append(values)
        for values in mutations:
            self.assertFalse(completed(0,output(values),'',args))

    def test_rejects_same_inputs_across_distinct_seeds(self):
        values = [record(name,SEEDS[0]) for name in PROPERTIES for _ in SEEDS]
        values = [(name,seed,value[2]) for value,(name,seed) in
                  zip(values,[(n,s) for n in PROPERTIES for s in SEEDS])]
        self.assertFalse(completed(0,output(values),'',SimpleNamespace(samples=4)))
