import json
import copy
import unittest
from types import SimpleNamespace
from test_native_base64url import encode
from native_date import completed, parse_wire, SEEDS

MINIMUM = (((1951,1,1),(0,0,0)),b'Mon, 01 Jan 1951 00:00:00 GMT')


def samples(seed):
    date = ((2000,8,7),(1,2,60))
    return [((date,b'Mon, 07 Aug 2000 01:02:60 GMT'),date),
            ((date,b'Monday, 07-Aug-00 01:02:60 GMT'),date),
            ((date,b'Mon Aug  7 01:02:60 2000'),date),
            ((((1951+seed[0]%100,1,1),(0,0,0)),
              ('Mon, 01 Jan %d 00:00:00 GMT'%(1951+seed[0]%100)).encode()),
             ((1951+seed[0]%100,1,1),(0,0,0)))]


def records():
    return [(seed,(samples(seed),[samples(seed)[0][0],MINIMUM],[MINIMUM])) for seed in SEEDS]


def output(values=None):
    values = records() if values is None else values
    return ''.join('COW_DATE_PROPER:'+json.dumps(list(bytes([131])+encode(value)))+'\n' for value in values)+'COW_DATE_PROPER_DONE\n'


class NativeDateTests(unittest.TestCase):
    def test_accepts_complete_seeded_property_results(self):
        self.assertTrue(completed(0,output(),'',SimpleNamespace(samples=4)))

    def test_rejects_crash_and_incomplete_or_corrupt_protocol(self):
        good = output()
        lines = good.splitlines()
        for code,out,err in [
            (1,good,''),(None,good,''),(0,'',''),
            (0,'COW_DATE_PROPER_DONE\n',''),
            (0,good+'extra\n',''),
            (0,'\n'.join(lines[:-1])+'\n',''),
            (0,'\n'.join([lines[0],lines[0],*lines[2:]])+'\n',''),
            (0,good,'Error in process <0.1.0>'),
            (0,good.replace('[131,','[true,',1),''),
            (0,good.replace('[131,','[256,',1),''),
            (0,good.replace('[131,','[0,',1),''),
        ]:
            self.assertFalse(completed(code,out,err,SimpleNamespace(samples=4)))

    def test_rejects_wrong_count_parsed_date_seed_or_reduction(self):
        mutations = []
        values = records(); values[0][1][0].pop(); mutations.append(values)
        values = records(); source,parsed=values[0][1][0][0]
        values[0][1][0][0]=(source,((1999,8,7),(1,2,60))); mutations.append(values)
        values = records(); values[0]=(SEEDS[1],values[0][1]); mutations.append(values)
        values = records(); values[0][1][1][0]=MINIMUM; mutations.append(values)
        values = records(); values[0][1][2][0]=values[0][1][1][0]; mutations.append(values)
        values = records(); source,parsed=values[0][1][0][0]
        values[0][1][0][0]=((source[0],b'not a date'),parsed); mutations.append(values)
        values = records(); values[0][1][0][:]=[values[0][1][0][0]]*4; mutations.append(values)
        for values in mutations:
            self.assertFalse(completed(0,output(values),'',SimpleNamespace(samples=4)))
        values = records()
        values = [(seed,copy.deepcopy(values[0][1])) for seed in SEEDS]
        self.assertFalse(completed(0,output(values),'',SimpleNamespace(samples=4)))

    def test_independent_oracle_century_padding_and_leap_second(self):
        self.assertEqual(parse_wire(b'Monday, 01-Jan-50 00:00:60 GMT'),
                         ('rfc850',((2050,1,1),(0,0,60))))
        self.assertEqual(parse_wire(b'Monday, 01-Jan-51 00:00:00 GMT'),
                         ('rfc850',((1951,1,1),(0,0,0))))
        self.assertEqual(parse_wire(b'Mon Jan  1 00:00:00 1951'),('asctime',MINIMUM[0]))
        # Upstream chooses components independently: calendar validity and
        # weekday consistency are deliberately outside this property's contract.
        self.assertEqual(parse_wire(b'Mon, 31 Feb 2000 00:00:00 GMT'),
                         ('fixdate',((2000,2,31),(0,0,0))))
        for wire in (b'Mon Jan 01 00:00:00 1951', b'Mon, 01 Jan 2051 00:00:00 GMT',
                     b'Mon, 00 Jan 2000 00:00:00 GMT', b'Mon, 01 Bad 2000 00:00:00 GMT',
                     b'Bad, 01 Jan 2000 00:00:00 GMT', b'Mon, 01 Jan 2000 00:00:61 GMT',
                     b'Mon, 01 Jan 2000 00:00:00 GMT extra', b'\xff'):
            with self.assertRaises(ValueError):parse_wire(wire)
