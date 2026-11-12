import json
from types import SimpleNamespace
import unittest
from test_native_base64url import encode
from native_http_hd import PROPERTIES, SEEDS, completed


def records():
    return [(name,seed,([(i+seed[0],True) for i in range(4)],[seed[0],0],[0]))
            for name in PROPERTIES for seed in SEEDS]


def output(values=None):
    values = records() if values is None else values
    def term(value):
        # ETF boolean atoms, distinct from integers in the protocol.
        if type(value)is bool:return bytes([119,4 if value else 5])+(b'true' if value else b'false')
        if type(value)is tuple:return bytes([104,len(value)])+b''.join(map(term,value))
        if type(value)is list:return bytes([108])+len(value).to_bytes(4,'big')+b''.join(map(term,value))+bytes([106])
        return encode(value)
    return ''.join('COW_HD_START:'+name+':'+','.join(map(str,seed))+'\n'+
                   'COW_HD_RESULT:'+json.dumps(list(bytes([131])+term((name,seed,value))))+'\n'
                   for name,seed,value in values)+'COW_HD_DONE\n'


class NativeHttpHdTests(unittest.TestCase):
    def test_accepts_all_seeded_property_results(self):
        args = SimpleNamespace(samples=4,properties=PROPERTIES)
        self.assertTrue(completed(0,output(),'',args))

    def test_rejects_failures_and_incomplete_or_duplicate_protocol(self):
        args=SimpleNamespace(samples=4,properties=PROPERTIES)
        good=output(); lines=good.splitlines()
        for code,out,err in [
            (1,good,''),(None,good,''),(0,'',''),
            (0,'COW_HD_DONE\n',''),(0,good+'extra\n',''),
            (0,'\n'.join(lines[:-1])+'\n',''),
            (0,'\n'.join([*lines[:2],*lines[:2],*lines[4:]])+'\n',''),
            (0,good,'Error in process <0.1.0>'),
            (0,good.replace('[131,','[true,',1),''),
            (0,good.replace('[131,','[256,',1),''),
            (0,good.replace('COW_HD_START:','WRONG:',1),''),
        ]:self.assertFalse(completed(code,out,err,args))

    def test_rejects_wrong_results_counts_seeds_and_controls(self):
        args=SimpleNamespace(samples=4,properties=PROPERTIES)
        mutations=[]
        values=records();values[0][2][0][0]=(7,False);mutations.append(values)
        values=records();values[0][2][0].pop();mutations.append(values)
        values=records();values[0]=(PROPERTIES[1],values[0][1],values[0][2]);mutations.append(values)
        values=records();values[0]=(values[0][0],SEEDS[1],values[0][2]);mutations.append(values)
        values=records();values[0][2][2][0]=-1;mutations.append(values)
        values=records();values[0][2][1].clear();mutations.append(values)
        values=records();values[0][2][0][:]=[(1,True)]*4;mutations.append(values)
        for values in mutations:self.assertFalse(completed(0,output(values),'',args))

    def test_has_exact_inventory_and_supports_explicit_subset(self):
        self.assertEqual(len(PROPERTIES),26)
        self.assertEqual(len(set(PROPERTIES)),26)
        args=SimpleNamespace(samples=4,properties=PROPERTIES[:1])
        self.assertTrue(completed(0,output(records()[:3]),'',args))
        self.assertFalse(completed(0,output(records()[:3]),'',SimpleNamespace(samples=4,properties=PROPERTIES)))

    def test_rejects_duplicate_property_selection(self):
        args=SimpleNamespace(samples=4,properties=(PROPERTIES[0],)*26)
        values=records()[:3]*26
        self.assertFalse(completed(0,output(values),'',args))
